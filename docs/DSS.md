# Drawen Style Sheets (DSS)

DSS is Drawen's CSS-inspired stylesheet format. It resolves visual declarations
into Drawen's typed style values; it does not expose stringly typed CSS concepts
to widgets or renderers.

The runtime applies DSS per retained node. Every node resolves a `StyleTarget`
built from its widget type, id, classes, and interaction state, then receives a
typed `DssStyle` before layout and again before paint. Widgets consume that
style directly, and inheritable text properties flow to descendants.

## Using a stylesheet

Pass source directly to `UiEnvironment::from_dss`, with an existing semantic
theme as a fallback:

```rust
use drawen::{DarkTheme, UiEnvironment};

let environment = UiEnvironment::from_dss(
    DarkTheme,
    r#"
        :root {
            background: #101820;
            color: #f1f3f5;
            accent-color: #2edcb3;
            gap: 12px;
            font-family: "Inter", "Noto Sans", sans-serif;
            font-size: 15px;
            font-weight: normal;
            border-radius: 6px;
        }

        button {
            background: #243447;
            padding: 6px 10px;
            font-weight: 600;
        }

        button:hover { background: #34516b; }
        button:pressed { background: #182838; }
        button:focus { border: 2px solid #2edcb3; }

        #save { background: #1f7a5a; }
        .primary { border-radius: 8px; }
        .danger { color: #ff6b6b; }
    "#,
)?;
```

The semantic theme remains the fallback for every value the stylesheet does not
supply. `:root` declarations become global theme tokens and the inheritance
root; type, id, class, and state rules stay as per-node styles. Adding DSS does
not require copying a complete palette, and a `button` rule no longer rewrites
the theme value used by unrelated widgets.

Inheritance covers text properties and `accent-color`; a `background` on
`:root` is a token, not a paint command for the window surface. Give the root
`Box` its own background (or style `box`) when the window should be filled.

For an end-to-end button example with `Styled`, `UiEnvironment::from_dss`, and
runtime-owned `:hover`/`:focus`, see
[RECIPES.md](RECIPES.md#4-a-stylesheet-with-dss).

## Supported selectors and cascade

DSS accepts a complex selector per rule, or a comma-separated list of them:

```dss
button { background: #243447; }
.primary { background: #336699; }
#save.primary:hover { background: rgba(12, 34, 56, 0.5); }
toolbar > button { padding: 2px 8px; }
.card .label { color: #8899aa; }
button:first-child { border-radius: 8px 0 0 8px; }
.panel > *:not(.spacer) { gap: 8px; }
button:hover, .preview { border: 2px solid #abcdef; }
```

A selector is a chain of compounds joined by combinators. The rightmost compound
matches the styled node; the others match its ancestors.

| Part | Meaning |
| --- | --- |
| `*` | Any non-root target. |
| `button` | A widget type name. |
| `#save` | A target id. |
| `.primary` | A target class. |
| `A B` | `B` with an ancestor matching `A`, at any depth. |
| `A > B` | `B` whose direct parent matches `A`. |
| `:hover`, `:pressed`, `:focus` | Runtime interaction state. |
| `:disabled`, `:checked`, `:selected` | Semantic state mirrored from accessibility. |
| `:first-child`, `:last-child`, `:only-child` | The node's position among its siblings. |
| `:nth-child(An+B)` | A position, accepting `odd`, `even`, `2`, `2n+1`, `-n+2`, and similar. |
| `:not(...)`, `:is(...)` | Compound argument lists, matched negatively or positively. |
| `:root` | The stylesheet root and global theme values. |

Matching declarations are merged by CSS specificity (id, then class/state, then
type) and source order. Combinators add the specificity of the compounds they
join, and `:not()`/`:is()` contribute the specificity of their strongest
argument. Rules are sorted once during parsing, so resolving a target does not
allocate or sort a per-node rule list.

Structural pseudo-classes need ancestor and sibling information, which the
runtime records while walking the tree: each node resolves its target with its
nearest-first parent chain and its one-based child position. Detached targets
have neither, so structural selectors do not match them.

Sibling combinators (`+` and `~`) are deliberately absent. They would require
per-node sibling metadata for every style pass, which does not pay for itself in
Drawen's retained tree.

## Custom properties, `calc()`, and `!important`

Rules may define custom properties and use them through `var()`:

```dss
:root {
    --space: 8px;
    --brand: #2edcb3;
    --line: 1px solid var(--brand);
}

button {
    padding: calc(var(--space) * 0.75) var(--space);
    border: var(--line);
    border-radius: var(--radius, 4px);
}
```

Custom properties are global stylesheet tokens rather than per-selector values:
the last definition in the file wins, regardless of the selector that carried
it. This is a deliberate simplification; it keeps a single typed token map and
avoids resolving variables per node. `var()` accepts a fallback after a comma,
fallbacks may themselves contain `var()`, and a missing token without a fallback
is a parse error. Cycles are rejected at parse time through a 32-level cap.

`calc()` accepts `+`, `-`, `*`, `/`, parentheses, `px` lengths, and unitless
numbers. Percentages, `em`, `vh`, and other units are rejected rather than
guessed at. A `calc()` used where a length is expected must produce a length;
numbers are only valid where a number is expected.

Declarations ending in `!important` form a second cascade layer. All important
declarations are applied after every normal declaration, so an important rule
beats a more specific normal rule, and important declarations among themselves
still resolve by specificity and source order.

## Per-node styling

Widgets identify themselves through `Widget::style_target()`:

```rust
use drawen::{PseudoClass, StyleTarget};

fn style_target(&self) -> StyleTarget {
    let target = StyleTarget::new("button");
    if self.pressed {
        target.with_state(PseudoClass::Pressed)
    } else {
        target
    }
}
```

The runtime augments that target with `:hover` and `:focus` from retained
input state. Standard widgets publish their `:pressed` or selection state where
relevant, and the accessibility flags `disabled`, `checked`/`mixed`, and
`selected` surface automatically as `:disabled`, `:checked`, and `:selected`.
During the style pass the runtime resolves each node's target,
inherits `color`, `font-family`, `font-size`, `font-weight`, `font-style`, and
`accent-color` from its parent, and stores the result on the node.
`LayoutContext::style()` exposes it while laying out; `paint_styled` receives it
while painting.

Use `Styled<W>` to add selectors without changing a widget's API:

```rust
use drawen::{Button, Styled};

let save = Styled::new(Button::new())
    .with_id("save")
    .with_class("primary")
    .with_class("danger");
```

`Styled` forwards layout, painting, events, and focus behavior while preserving
the wrapped widget's type name, so the button above matches `button`,
`#save`, `.primary`, and `.danger`.

Custom widgets can resolve a style through the same contract with
`UiEnvironment::style_for(&target)`, or read `LayoutContext::style()` for the
node currently being laid out. There is no separate path that silently drops a
declaration.

Standard widget type names include `box`, `text`, `button`, `checkbox`,
`radio`, `slider`, `text-field`, `progress-bar`, `menu-bar`, `toolbar`,
`tab-bar`, `dropdown`, `splitter`, `dock-area`, `list-view`, `scroll-view`,
`padding`, `align`, `sized-box`, `image`, and `stack`.

## Supported properties

| Property | Value | Applied to |
| --- | --- | --- |
| `background`, `background-color` | `transparent`, a CSS Color Level 4 name, `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(...)`, `rgba(...)`, `hsl(...)`, or `hsla(...)`. | Surfaces such as boxes, buttons, fields, track backgrounds, bars, and docks. |
| `color` | A supported color. | Text and glyphs; inherits to descendants. |
| `accent-color` | A supported color. | Focus, selection, caret, divider, and filled-control values; inherits to descendants. |
| `border` | `<length> solid <color>`. | Surface outlines. |
| `padding` | One to four lengths. | Inner spacing of boxes, buttons, fields, toolbars, and similar containers. |
| `gap` | A non-negative length. | Spacing between labels and controls or toolbar items. |
| `font-family` | A comma-separated list of family names, optionally quoted. | Text shaped during layout; inherits to descendants. |
| `font-size` | A non-negative `px` or `pt` length. | Text shaped during layout; inherits to descendants. |
| `font-weight` | `normal`, `bold`, or an integer from 100 through 900. | Text shaped during layout; inherits to descendants. |
| `font-style` | `normal`, `italic`, or `oblique`. | Text shaped during layout; inherits to descendants. |
| `border-radius` | A non-negative length. | Rounded surfaces. |
| `width`, `height` | A non-negative length. | The node's own size, clamped to what the parent offers. |
| `min-width`, `min-height`, `max-width`, `max-height` | A non-negative length. | Bounds on the node's size; minimums win over maximums. |

Non-zero lengths require the `px` unit. DSS accepts `0` without a unit. Any
length also accepts `calc(...)`.

Colors accept the CSS Color Level 4 palette, so `rebeccapurple`, `tomato`, or
`steelblue` work alongside hex and functional forms. `rgb()` and `hsl()` take
either comma-separated channels or the modern space-separated form with an
optional `/ alpha`:

```dss
.panel { background: rgb(12 34 56 / 0.5); }
.badge { background: hsl(168 76% 52%); }
```

RGB channels are numbers from `0` through `255` or percentages. Hue is a number
of degrees, optionally suffixed with `deg`, while saturation and lightness are
percentages. The
alpha component is a number from `0` through `1` or a percentage. Comments use
the familiar `/* comment */` form.

Size properties are applied by the runtime while laying out the parent: it
tightens or clamps the constraints it offers the child, so `width: 320px` never
asks for geometry the parent cannot grant. `min-*` takes precedence over `max-*`
and over an explicit `width`/`height` when the parent's range makes them
conflict. Widgets remain unaware of DSS and simply receive a tighter range.

Font properties feed Drawen's Qt-style font model: `font-family` becomes an
ordered family preference list, `font-size` keeps its unit so point sizes can be
resolved at the environment's logical DPI, and weight/style participate in face
selection. See [`FONTS.md`](FONTS.md) for the request/face model, fallback rules,
and how to register additional faces.

Unknown properties, undefined custom properties, and unsupported syntax produce
`DssError`; Drawen never silently drops a style declaration. Selectors must be a
valid sequence of compounds and combinators: `*button` is rejected rather than
silently treated as `button`, while `*.primary` remains valid, and a trailing
combinator such as `box >` is an error.

## Out of scope

The parser has no sibling combinators, percentages, media queries, keyframes,
animations, or full browser-CSS compatibility. Inheritance covers `color`,
`font-family`, `font-size`, `font-weight`, `font-style`, and `accent-color`;
surface properties do not inherit. Drawen keeps this deliberately small typed
subset rather than emulating a browser engine: every declaration is validated
and converted into typed values before a widget sees it.
