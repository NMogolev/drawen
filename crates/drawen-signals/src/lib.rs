//! Typed synchronous notifications for Drawen.
//!
//! [`Signal`] is a value-less notification channel: widgets and application
//! models emit payloads, listeners receive them synchronously on the same
//! thread. [`Subscription`] is the drop-based connection handle; dropping it
//! disconnects the listener immediately.
//!
//! Everything here is UI-thread-local (`Rc`/`RefCell`, never `Send`/`Sync`).
//! There is no global event bus, no queued delivery, and no dependency graph:
//! a signal notifies exactly the listeners registered when emission starts.

mod signal;

pub use signal::{Signal, Subscription};
