//! Graphics-free eyes and hands for LLM agents.
//!
//! Drawen already describes every widget semantically for assistive technology:
//! roles, names, values, states, actions, and bounds live in the runtime's
//! [`SemanticTree`]. This crate turns that tree into the observation channel a
//! language model actually wants — compact text or JSON instead of pixels —
//! and turns model output back into real input. No screenshot, no renderer,
//! no GPU: the agent reads the same semantics a screen reader would, and its
//! clicks run through the same action and event paths as a human's.
//!
//! # Observe and act on the UI thread
//!
//! [`Agent`] borrows the runtime per call. Every reply carries a fresh
//! snapshot, so one round trip is a complete agent step: act, then see.
//!
//! ```
//! use drawen::{Button, Color, Constraints, Size, Text, UiRuntime, render::TextStyle};
//! use drawen_agent::{Agent, Command, Target};
//!
//! let mut runtime = UiRuntime::new(drawen::Box::new());
//! let button = runtime.append_child(runtime.root(), Button::new()).unwrap();
//! runtime
//!     .append_child(button, Text::styled("Save", TextStyle::new(14.0, Color::BLACK)))
//!     .unwrap();
//! runtime
//!     .layout(Constraints::tight(Size::new(320.0, 200.0).unwrap()).unwrap())
//!     .unwrap();
//!
//! let agent = Agent::new();
//! println!("{}", agent.observe(&runtime).to_text());
//! let reply = agent.execute(
//!     &mut runtime,
//!     &Command::Click { target: Target::Name("Save".to_owned()) },
//! );
//! assert!(reply.ok && reply.handled);
//! ```
//!
//! # Serve an external agent process
//!
//! With feature `serve`, [`AgentServer`] listens on loopback and speaks JSON
//! lines: one command per line in, one reply per line out. Socket work stays
//! on a [`drawen_tasks::TaskPool`]; commands reach the UI thread through a
//! dispatcher the application drains once per event-loop turn. The widget
//! tree, signals, and subscriptions never leave the UI thread.
//!
//! ```json
//! {"op":"observe"}
//! {"op":"click","target":"Save"}
//! {"op":"set_value","target":{"role":"text_field","label":"Name"},"value":"Grace"}
//! {"op":"press_key","key":"enter"}
//! ```
//!
//! See `docs/AGENT.md` in the repository for the full protocol, a ready-made
//! model prompt, and the headless and native-window examples.
//!
//! # Features
//!
//! - `json` (default): serde support for snapshots, commands, and replies.
//! - `serve`: the loopback TCP bridge; implies `json`.

mod agent;
mod command;
#[cfg(feature = "serve")]
mod serve;
mod snapshot;

pub use agent::{Agent, Reply};
pub use command::{Command, Modifier, Target};
#[cfg(feature = "serve")]
pub use serve::AgentServer;
pub use snapshot::{Snapshot, SnapshotNode, role_name};
