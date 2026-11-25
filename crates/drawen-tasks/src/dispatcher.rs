use core::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender},
};

/// Approximate number of values that have been sent but not received yet.
#[derive(Clone, Default)]
struct QueueDepth(Arc<AtomicUsize>);

impl QueueDepth {
    fn increase(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }

    fn decrease(&self, amount: usize) {
        // `AtomicUsize::try_update` replaced `fetch_update` in newer Rust, but
        // the workspace still supports 1.88, so keep the compare-exchange loop
        // instead of the deprecated method.
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let next = current.saturating_sub(amount);
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// The worker-side half of a [`UiDispatcher`].
///
/// The sender is clonable, `Send`, and `Sync`, so it can be captured by any
/// number of tasks. Wake-ups are coalesced: the wake callback supplied to
/// [`UiDispatcher::new`] runs once per pending batch, so a burst of sends
/// nudges the event loop once instead of once per value. The callback should
/// only nudge the event loop, not run application logic.
pub struct UiSender<T> {
    sender: Sender<T>,
    wake: Arc<dyn Fn() + Send + Sync>,
    wake_pending: Arc<AtomicBool>,
    depth: QueueDepth,
}

impl<T> UiSender<T> {
    /// Sends an owned value to the UI thread and wakes it.
    ///
    /// The send itself never blocks. The value becomes available on the UI
    /// thread through [`UiDispatcher::drain`] or
    /// [`UiDispatcher::try_recv`]. While a wake-up is still pending, further
    /// sends queue their values without invoking the wake callback again.
    ///
    /// # Errors
    ///
    /// Returns [`UiSendError`] with the value back when the dispatcher has
    /// already been dropped, for example because the window closed.
    pub fn send(&self, value: T) -> Result<(), UiSendError<T>> {
        self.depth.increase();
        match self.sender.send(value) {
            Ok(()) => {
                if !self.wake_pending.swap(true, Ordering::SeqCst) {
                    (self.wake)();
                }
                Ok(())
            }
            Err(error) => {
                self.depth.decrease(1);
                Err(UiSendError(error.0))
            }
        }
    }
}

impl<T> Clone for UiSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
            wake_pending: Arc::clone(&self.wake_pending),
            depth: self.depth.clone(),
        }
    }
}

impl<T> fmt::Debug for UiSender<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("UiSender").finish_non_exhaustive()
    }
}

/// A value that could not be delivered because the UI side was gone.
pub struct UiSendError<T>(T);

impl<T> UiSendError<T> {
    /// Returns the undelivered value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for UiSendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiSendError")
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Display for UiSendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the UI dispatcher has been dropped")
    }
}

impl<T: fmt::Debug> std::error::Error for UiSendError<T> {}

/// The UI-thread half of the worker-to-UI bridge.
///
/// Drawen's widget tree, [`Signal`](https://docs.rs/drawen/latest/drawen/struct.Signal.html)s,
/// and subscriptions are UI-thread-local. This type is the boundary: workers
/// hold a [`UiSender`], which can move owned data across threads, and the UI
/// thread drains it during `WindowApplication::window_event` or
/// `next_frame_in` and applies the values to application state.
///
/// Create the pair with [`Self::new`], keep the dispatcher on the UI thread,
/// and give the sender to [`TaskPool`](crate::TaskPool) tasks. The dispatcher
/// is `Send` but not `Sync`; unlike the sender it is not meant to be shared
/// with workers.
///
/// # Example
///
/// ```
/// use drawen_tasks::UiDispatcher;
///
/// let (dispatcher, sender) = UiDispatcher::new(|| {
///     // Wake the native event loop here, for example with an
///     // EventLoopProxy.
/// });
///
/// sender.send(String::from("decoded frame")).expect("dispatcher is alive");
///
/// // On the UI thread, after the wake-up arrives:
/// assert_eq!(dispatcher.drain(), vec![String::from("decoded frame")]);
/// ```
pub struct UiDispatcher<T> {
    receiver: Receiver<T>,
    sender: Sender<T>,
    wake: Arc<dyn Fn() + Send + Sync>,
    wake_pending: Arc<AtomicBool>,
    depth: QueueDepth,
}

impl<T> UiDispatcher<T> {
    /// Creates a dispatcher and its worker-side sender.
    ///
    /// `wake` is invoked when a send arrives while no wake-up is pending;
    /// bursts of sends coalesce into one call. It should perform a cheap
    /// thread-safe wake-up, such as sending a user event to the native event
    /// loop; it must not touch the widget tree. Receiving values re-arms the
    /// wake-up, so values sent while the UI thread is draining are never
    /// stranded without a wake.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> (Self, UiSender<T>) {
        let (sender, receiver) = mpsc::channel();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let wake_pending = Arc::new(AtomicBool::new(false));
        let depth = QueueDepth::default();
        let dispatcher = Self {
            receiver,
            sender: sender.clone(),
            wake: Arc::clone(&wake),
            wake_pending: Arc::clone(&wake_pending),
            depth: depth.clone(),
        };
        let ui_sender = UiSender {
            sender,
            wake,
            wake_pending,
            depth,
        };
        (dispatcher, ui_sender)
    }

    /// Returns another worker-side sender for this dispatcher.
    ///
    /// Keeping the returned handle means the channel stays open even when
    /// every temporary sender has been dropped.
    #[must_use]
    pub fn sender(&self) -> UiSender<T> {
        UiSender {
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
            wake_pending: Arc::clone(&self.wake_pending),
            depth: self.depth.clone(),
        }
    }

    /// Clears the pending wake-up and re-arms it when values remain queued.
    ///
    /// A sender that observed the pending flag skips its wake-up, so after
    /// receiving, the dispatcher must check for values that arrived while it
    /// was draining and wake the loop for them itself.
    fn rearm_wake(&self) {
        self.wake_pending.store(false, Ordering::SeqCst);
        if self.depth.get() > 0 && !self.wake_pending.swap(true, Ordering::SeqCst) {
            (self.wake)();
        }
    }

    /// Returns the next queued value without blocking.
    #[must_use]
    pub fn try_recv(&self) -> Option<T> {
        let value = self.receiver.try_recv().ok();
        if value.is_some() {
            self.depth.decrease(1);
            self.rearm_wake();
        }
        value
    }

    /// Removes and returns every queued value.
    ///
    /// Call this once per event-loop turn after the wake-up, apply the values
    /// to application state, then invalidate paint and request one redraw.
    #[must_use]
    pub fn drain(&self) -> Vec<T> {
        let values: Vec<T> = self.receiver.try_iter().collect();
        self.depth.decrease(values.len());
        self.rearm_wake();
        values
    }

    /// Appends every queued value to `buffer`, reusing its allocation.
    pub fn drain_into(&self, buffer: &mut Vec<T>) {
        let before = buffer.len();
        buffer.extend(self.receiver.try_iter());
        self.depth.decrease(buffer.len() - before);
        self.rearm_wake();
    }

    /// Returns whether no values are waiting.
    ///
    /// A value that a worker is in the middle of sending may not be counted
    /// yet; use [`Self::drain`] to observe the authoritative set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.depth.get() == 0
    }

    /// Returns the approximate number of queued values.
    ///
    /// The count can be one higher than the number that [`Self::drain`] would
    /// return while a send is in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.depth.get()
    }
}

impl<T> fmt::Debug for UiDispatcher<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiDispatcher")
            .field("queued", &self.len())
            .finish_non_exhaustive()
    }
}
