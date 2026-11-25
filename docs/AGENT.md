# Let an agent see and drive the UI

`drawen-agent` gives a language model native eyes and hands for a Drawen
application. No screenshots, no renderer, no GPU: the model reads the same
semantic tree a screen reader reads — roles, names, values, states, actions,
bounds — and its commands run through the same accessibility and input paths a
human's clicks and keystrokes use. Application signal handlers cannot tell the
difference.

The crate is small and layered:

- `Agent` — UI-thread session: `observe(&runtime)` and
  `execute(&mut runtime, &command)`. Every reply carries a fresh snapshot, so
  one round trip is a complete agent step: act, then see the result.
- `Snapshot` — plain, `Send` data. Renders as compact JSON (feature `json`,
  default) or as a token-efficient text outline. Observations are bounded so a
  large tree never floods the model's context.
- `AgentServer` (feature `serve`) — a loopback TCP endpoint speaking JSON
  lines, so an external model process can drive the app. Socket work stays on
  a `TaskPool`; commands cross to the UI thread through a `UiDispatcher`.

Enable it from the facade with the `agent` feature, or `agent-serve` for the
TCP endpoint. `drawen-agent` is released in lockstep with the facade and
follows its version; the release checklist semver-checks it alongside the
other crates.

## Quick start

Headless — no window, no display server:

```rust
use drawen::{Box as DrawenBox, UiRuntime};
use drawen::agent::AgentServer;

let mut runtime = UiRuntime::new(DrawenBox::new());
// ... append widgets, lay out once ...
let mut server = AgentServer::bind(9870)?;
loop {
    if server.drain(&mut runtime) {
        let _ = runtime.take_redraw_request();
    }
    std::thread::sleep(std::time::Duration::from_millis(10));
}
```

Native window — the canonical `background_tasks` pattern:

```rust
fn attach_waker(&mut self, waker: WindowWaker) {
    self.server.attach_waker(move || waker.wake());
}

// in window_event:
WindowEvent::RedrawRequested => {
    if self.server.drain(&mut self.runtime) {
        self.runtime.invalidate_paint();
        return WindowAction::RequestRedraw;
    }
    WindowAction::None
}
```

Runnable examples:

- `cargo run -p drawen-agent --example headless_agent` — a windowless app an
  agent drives over TCP.
- `cargo run -p drawen-agent --example agent_window` — a normal native window
  that an agent can also drive.

## What the agent sees

The text outline costs few prompt tokens:

```text
focus=#3 total=7
#0 generic_container @(0,0,360,240)
  #1 generic_container @(0,0,360,120)
    #2 text_field value="Grace" [focus,set_value,select] @(0,0,200,28)
    #3 checkbox "Subscribe" checked [focus,click,toggle] @(0,28,120,24)
    #5 button "Save" [focus,click] @(0,52,64,28)
```

`#id` is the widget's arena index. Bounds are absolute logical pixels
`@(x,y,width,height)`. States appear as words (`focused`, `checked`,
`disabled`, `expanded`, `selected`, …); supported actions appear in brackets.
The JSON form carries the same data plus `parent`/`children` links, numeric
values with bounds, and descriptions.

Only laid-out, visible widgets appear. A control's name is derived from its
child text when not set explicitly, exactly as for assistive technology.

### Snapshot size is a contract

A virtualized list on a tall window still exposes a hundred visible rows, and
nested containers can reach thousands of nodes. An unbounded snapshot would
spend the agent's whole context on the UI and leave nothing for reasoning, so
observations are bounded:

- Every snapshot keeps at most `max_nodes` nodes (default
  `DEFAULT_MAX_NODES = 200`) in depth-first tree order.
- `total` counts every node in the observed scope; `truncated: true` (the word
  `truncated` in the text header) says the budget cut some off.
- `{"op":"observe","target":<id>}` observes only that widget's subtree. Ids
  stay global arena indices, so any listed node remains directly targetable.
- `{"op":"observe","max_nodes":50}` overrides the budget for one observation.

When a snapshot is truncated, observe subtrees by id instead of re-reading the
root — the same discipline as watching one layout region instead of the whole
window.

## Protocol

One JSON command per line in, one JSON reply per line out. Replies are
strictly request-response: the server never pushes unsolicited messages, so a
client that writes one line and reads one line stays correct.

Several connections may be open at once. Every command from every connection
is serialized onto the UI thread in arrival order; within one connection
commands are strictly sequential.

| Command | Example |
| --- | --- |
| `observe` | `{"op":"observe"}` |
| `observe` (subtree) | `{"op":"observe","target":7}` |
| `observe` (budget) | `{"op":"observe","target":7,"max_nodes":50}` |
| `click` | `{"op":"click","target":"Save"}` |
| `toggle` | `{"op":"toggle","target":"Subscribe"}` |
| `focus` | `{"op":"focus","target":3}` |
| `select` | `{"op":"select","target":2}` |
| `set_value` | `{"op":"set_value","target":2,"value":"Grace"}` |
| `increment` / `decrement` | `{"op":"increment","target":7}` |
| `expand` / `collapse` | `{"op":"expand","target":7}` |
| `scroll` | `{"op":"scroll","target":7,"dx":0.0,"dy":-120.0}` |
| `press_key` | `{"op":"press_key","key":"enter","modifiers":["ctrl"]}` |
| `type_text` | `{"op":"type_text","text":"hello"}` |
| `click_at` | `{"op":"click_at","x":120.0,"y":90.0}` |
| `advance` | `{"op":"advance","ms":250.0}` |
| `wait_until_idle` | `{"op":"wait_until_idle","timeout_ms":2000.0}` |

Targets resolve against the current tree:

- a number is an arena index from the latest snapshot;
- a string is a case-insensitive accessible-name substring;
- an object `{"role":"button","label":"Save"}` filters by role and/or label.
  Role names are forgiving: `textfield`, `text_field`, and `TextField` match.

An exact name wins over substrings. A unique substring match is accepted. An
ambiguous query fails with the candidate list instead of guessing, so the
model can retry by id. A stale index fails instead of hitting a recycled slot.

Case-insensitivity uses Unicode lowercasing, not full case folding: `İ`
matches `i̇` as lowercased, but `ß` does not match `ss`. For tricky non-ASCII
labels prefer exact names or ids.

`press_key` names: `backspace`, `enter`, `escape`, `tab`, `space`, `arrowleft`
(or `left`), `arrowright`, `arrowup`, `arrowdown`, `home`, `end`, `pageup`,
`pagedown`, `delete`. Any single Unicode scalar value is a text key;
multi-scalar sequences (combining marks, joined emoji) go through `type_text`
instead. Modifiers: `ctrl`, `shift`, `alt`, `meta`. Keys route through the
real event router, so Tab traversal, Escape overlay dismissal, and registered
commands all work.

`click_at` synthesizes a primary pointer click at window coordinates — the
fallback for custom widgets without semantic actions. Prefer `click` whenever
the snapshot lists a `click` action.

Replies:

```json
{"ok":true,"handled":true,"snapshot":{...}}
{"ok":false,"handled":false,"error":"no widget matches label \"Svae\"","snapshot":{...}}
```

- `ok` — the command was well-formed and its target resolved.
- `handled` — the runtime or widget actually performed the interaction.
  `ok` with `handled:false` means the target declined, e.g. a disabled button
  refuses both `click` and `focus`, or `wait_until_idle` timed out. The flag
  is consistent across all refusing actions.
- `snapshot` — the post-command observation, always present except for
  transport-level errors (malformed JSON lines). An `observe` reply carries
  exactly the requested subtree and budget; other commands reply with the
  default whole-tree observation.

Errors are human-readable strings today; structured error codes are a possible
future addition, so parse `ok`/`handled` programmatically and treat `error` as
text.

## Controlling time

A click reply arrives immediately even when the UI is mid-transition: a toast
fading in, an expander opening, a scroll settling. Reading that transient state
makes an agent "fix" things that are not broken. Two commands give the agent a
controlled, deterministic clock:

- `{"op":"advance","ms":250}` steps the animation clock by a fixed amount in
  60 Hz frames, then replies with the settled snapshot. Budgets are capped at
  60 seconds per command.
- `{"op":"wait_until_idle"}` advances time until no animation frames remain
  scheduled (`timeout_ms` defaults to 5000, capped at 60000). `handled:true`
  means the tree settled; `handled:false` means the budget ran out — an
  indeterminate spinner or playing media keeps looping forever, and that is
  reported instead of hanging.

The clock is the runtime's own `tick`, so media clocks, property damage, and
widget animations advance exactly as they would in a presented window. After
any action whose result animates, `advance` or `wait_until_idle` before judging
the outcome.

## Threading contract

The widget tree, signals, and subscriptions stay UI-thread-only. The server's
socket workers own plain data; commands cross through `UiDispatcher`, and the
application applies them in `drain` on the UI thread. `Agent::execute` calls
`UiRuntime::ensure_layout` before snapshotting so bounds reflect the new state
without a paint pass.

## Security

The listener binds `127.0.0.1` only and has no authentication. **The agent has
the same authority as a human at the keyboard. Do not enable this in a session
you would not hand the keyboard to.** On a shared machine, loopback is not a
security boundary: any local process — another application, a browser
extension's native host, co-installed malware — can drive the UI while the
server is enabled. Enable it only while an agent is supposed to control the
application, and drop the server when it is done. Command lines are capped at
1 MiB. An optional token/capability handshake is a possible future addition
for hostile multi-process environments.

## Out of scope

Semantic observation answers "does the button exist, is it enabled, is it
named Save" — not "is the button the right shade of blue". Visual verification
against a design mockup is deliberately out of scope: navigation, input, and
state checks are what the semantic tree is for. A future
`{"op":"display_list","region":...}` could expose a subset of the runtime's
display list so an agent diffs drawing primitives instead of pixels — no
screenshot needed — but that is not part of the current protocol.

## A prompt for the model

Paste this into the agent's system prompt, with the endpoint address:

```text
You control a native Drawen application through a JSON-lines TCP endpoint at
127.0.0.1:9870. Send one JSON command per line; each reply contains "ok",
"handled", and a fresh "snapshot" of the UI. Replies are strictly
request-response; the server never pushes events.

The snapshot lists widgets as #id with a role, an optional quoted name, a
value, state words, [actions], and @(x,y,w,h) bounds. The header reports
total=<node count> and the word truncated when the node budget cut the tree.
If a snapshot is truncated, observe subtrees with {"op":"observe","target":<id>}
instead of re-reading the root; ids stay valid across subtree views.

Prefer semantic actions:
{"op":"click","target":"Save"} activates a button by name;
{"op":"set_value","target":2,"value":"..."} writes a field;
{"op":"toggle","target":"Subscribe"} flips a checkbox;
{"op":"press_key","key":"enter"} presses a key on the focused widget.
Use {"op":"observe"} to look without acting. Target by #id when names are
ambiguous; an ambiguous name returns the candidate list.

The UI animates. After an action whose result transitions (toasts, expanders,
scrolls), send {"op":"advance","ms":250} or {"op":"wait_until_idle"} before
judging the outcome; wait_until_idle reports handled:false if a live loop
(a spinner, playing media) never settles. Read the snapshot after every
action instead of assuming the result. ok:true with handled:false means the
target declined (e.g. disabled). Do not use click_at unless the target
exposes no actions.
```

A minimal Python client (one line out, one line back — no server push):

```python
import json, socket

with socket.create_connection(("127.0.0.1", 9870)) as sock:
    stream = sock.makefile("rwb")
    def send(command):
        stream.write((json.dumps(command) + "\n").encode())
        stream.flush()
        return json.loads(stream.readline())

    print(send({"op": "observe"})["snapshot"])
    print(send({"op": "click", "target": "Save"}))
    print(send({"op": "wait_until_idle"}))
```

## Relationship to native accessibility

`drawen-agent` consumes the same `SemanticTree` that
`SemanticTree::to_accesskit` publishes through UI Automation, AT-SPI, and the
macOS Accessibility API. Custom widgets participate automatically by
implementing `Widget::accessibility` and `Widget::accessibility_action`; no
agent-specific code is needed in widgets.