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
  default) or as a token-efficient text outline.
- `AgentServer` (feature `serve`) — a loopback TCP endpoint speaking JSON
  lines, so an external model process can drive the app. Socket work stays on
  a `TaskPool`; commands cross to the UI thread through a `UiDispatcher`.

Enable it from the facade with the `agent` feature, or `agent-serve` for the
TCP endpoint.

## Quick start

Headless — no window, no display server:

```rust
use drawen::agent::AgentServer;

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
focus=#3
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

## Protocol

One JSON command per line in, one JSON reply per line out.

| Command | Example |
| --- | --- |
| `observe` | `{"op":"observe"}` |
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

Targets resolve against the current tree:

- a number is an arena index from the latest snapshot;
- a string is a case-insensitive accessible-name substring;
- an object `{"role":"button","label":"Save"}` filters by role and/or label.
  Role names are forgiving: `textfield`, `text_field`, and `TextField` match.

An exact name wins over substrings. A unique substring match is accepted. An
ambiguous query fails with the candidate list instead of guessing, so the
model can retry by id. A stale index fails instead of hitting a recycled slot.

`press_key` names: `backspace`, `enter`, `escape`, `tab`, `space`, `arrowleft`
(or `left`), `arrowright`, `arrowup`, `arrowdown`, `home`, `end`, `pageup`,
`pagedown`, `delete`; any single character is a text key. Modifiers: `ctrl`,
`shift`, `alt`, `meta`. Keys route through the real event router, so Tab
traversal, Escape overlay dismissal, and registered commands all work.

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
  `ok` with `handled:false` means the target declined, e.g. a disabled button.
- `snapshot` — the post-command observation, always present except for
  transport-level errors (malformed JSON lines).

## Threading contract

The widget tree, signals, and subscriptions stay UI-thread-only. The server's
socket workers own plain data; commands cross through `UiDispatcher`, and the
application applies them in `drain` on the UI thread. `Agent::execute` calls
`UiRuntime::ensure_layout` before snapshotting so bounds reflect the new state
without a paint pass.

## Security

The listener binds `127.0.0.1` only and has no authentication: any local
process can drive the UI while it is enabled. Keep it on loopback and enable
it only while an agent is supposed to control the application. Command lines
are capped at 1 MiB.

## A prompt for the model

Paste this into the agent's system prompt, with the endpoint address:

```text
You control a native Drawen application through a JSON-lines TCP endpoint at
127.0.0.1:9870. Send one JSON command per line; each reply contains "ok",
"handled", and a fresh "snapshot" of the UI.

The snapshot lists widgets as #id with a role, an optional quoted name, a
value, state words, [actions], and @(x,y,w,h) bounds. Prefer semantic actions:
{"op":"click","target":"Save"} activates a button by name;
{"op":"set_value","target":2,"value":"..."} writes a field;
{"op":"toggle","target":"Subscribe"} flips a checkbox;
{"op":"press_key","key":"enter"} presses a key on the focused widget.
Use {"op":"observe"} to look without acting. Target by #id when names are
ambiguous; an ambiguous name returns the candidate list. Read the snapshot
after every action instead of assuming the result. Do not use click_at unless
the target exposes no actions.
```

A minimal Python client:

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
```

## Relationship to native accessibility

`drawen-agent` consumes the same `SemanticTree` that
`SemanticTree::to_accesskit` publishes through UI Automation, AT-SPI, and the
macOS Accessibility API. Custom widgets participate automatically by
implementing `Widget::accessibility` and `Widget::accessibility_action`; no
agent-specific code is needed in widgets.
