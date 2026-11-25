//! Local TCP bridge between an external agent process and the UI thread.
//!
//! [`AgentServer`] listens on loopback and speaks JSON lines: one [`Command`]
//! per line in, one [`Reply`] per line out. Socket work runs on a
//! [`TaskPool`]; commands cross to the UI thread through a [`UiDispatcher`],
//! and the application drains them with [`AgentServer::drain`] exactly like
//! any other worker delivery. The widget tree never leaves the UI thread.
//!
//! The listener binds `127.0.0.1` only. The protocol has no authentication,
//! so any local process can drive the UI; keep it on loopback and enable it
//! only while an agent is supposed to control the application.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use drawen_tasks::{TaskPool, UiDispatcher, UiSender};
use drawen_ui::UiRuntime;

use crate::{Agent, Command, Reply};

/// Largest accepted command line; longer lines are rejected and the
/// connection closes.
const MAX_LINE: usize = 1 << 20;
/// Idle poll interval for the non-blocking accept loop and shutdown waits.
const POLL: Duration = Duration::from_millis(20);
/// Read timeout so connection workers notice shutdown promptly.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

struct Job {
    command: Command,
    reply: mpsc::Sender<Reply>,
}

/// A loopback JSON-lines endpoint for LLM agents.
///
/// Create it with [`Self::bind`], hand the event-loop waker to
/// [`Self::attach_waker`], and call [`Self::drain`] on the UI thread once per
/// event-loop turn (the canonical place is `WindowEvent::RedrawRequested`).
/// Dropping the server stops the listener and joins its workers.
pub struct AgentServer {
    agent: Agent,
    // Declared before `pool` and taken in `Drop` so queued jobs release their
    // reply channels before workers are joined.
    dispatcher: Option<UiDispatcher<Job>>,
    pool: Arc<TaskPool>,
    stop: Arc<AtomicBool>,
    wake_slot: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>,
    local: SocketAddr,
}

impl AgentServer {
    /// Binds `127.0.0.1:{port}` and starts serving.
    ///
    /// Pass port `0` to let the operating system pick a free port; read it
    /// back with [`Self::local_addr`].
    ///
    /// # Errors
    ///
    /// Returns the operating-system error when the socket cannot bind or the
    /// worker threads cannot start.
    pub fn bind(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let wake_slot: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>> = Arc::new(OnceLock::new());
        let slot = Arc::clone(&wake_slot);
        let (dispatcher, sender) = UiDispatcher::new(move || {
            if let Some(wake) = slot.get() {
                wake();
            }
        });
        let pool = Arc::new(TaskPool::with_thread_name(8, "drawen-agent")?);
        let stop = Arc::new(AtomicBool::new(false));
        let accept_pool = Arc::clone(&pool);
        let accept_sender = sender.clone();
        let accept_stop = Arc::clone(&stop);
        pool.spawn(move || accept_loop(&listener, &accept_pool, &accept_sender, &accept_stop));
        Ok(Self {
            agent: Agent::new(),
            dispatcher: Some(dispatcher),
            pool,
            stop,
            wake_slot,
            local,
        })
    }

    /// Returns the bound loopback address.
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Stores the event-loop wake-up used when a command arrives.
    ///
    /// Call once from `WindowApplication::attach_waker`, usually as
    /// `server.attach_waker(move || waker.wake())`. Before a waker is attached
    /// commands still queue; a headless application that polls [`Self::drain`]
    /// never needs one. The first waker wins.
    pub fn attach_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake_slot.set(Box::new(wake));
    }

    /// Executes every queued command on the UI thread and replies to clients.
    ///
    /// Returns whether any command changed state, so the caller can invalidate
    /// paint and request one redraw. Call once per event-loop turn.
    pub fn drain(&mut self, runtime: &mut UiRuntime) -> bool {
        let Some(dispatcher) = &self.dispatcher else {
            return false;
        };
        let mut changed = false;
        for job in dispatcher.drain() {
            let reply = self.agent.execute(runtime, &job.command);
            changed |= reply.handled;
            let _ = job.reply.send(reply);
        }
        changed
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Close the queue so workers blocked on a reply observe the shutdown.
        self.dispatcher = None;
        // The accept loop holds the only other pool reference; it exits within
        // one poll interval. Connection workers then finish within a read
        // timeout, and dropping the pool joins them.
        while Arc::strong_count(&self.pool) > 1 {
            std::thread::sleep(POLL);
        }
    }
}

fn accept_loop(
    listener: &TcpListener,
    pool: &Arc<TaskPool>,
    sender: &UiSender<Job>,
    stop: &Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let sender = sender.clone();
                let stop = Arc::clone(stop);
                pool.spawn(move || serve_connection(stream, &sender, &stop));
            }
            Err(_) => std::thread::sleep(POLL),
        }
    }
}

fn serve_connection(stream: TcpStream, sender: &UiSender<Job>, stop: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while !stop.load(Ordering::Relaxed) {
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                // Keep any partial line; the rest arrives on a later read.
                continue;
            }
            Err(_) => break,
        }
        if line.len() > MAX_LINE {
            let _ = write_reply(&mut writer, &Reply::error("command line exceeds 1 MiB"));
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }
        let reply = match serde_json::from_str::<Command>(trimmed) {
            Ok(command) => {
                let (tx, rx) = mpsc::channel();
                if sender.send(Job { command, reply: tx }).is_err() {
                    let _ = write_reply(&mut writer, &Reply::error("the UI thread is gone"));
                    break;
                }
                match rx.recv() {
                    Ok(reply) => reply,
                    Err(_) => break,
                }
            }
            Err(error) => Reply::error(format!("invalid command: {error}")),
        };
        if write_reply(&mut writer, &reply).is_err() {
            break;
        }
        line.clear();
    }
}

fn write_reply(writer: &mut TcpStream, reply: &Reply) -> io::Result<()> {
    let mut json = serde_json::to_string(reply).map_err(io::Error::other)?;
    json.push('\n');
    writer.write_all(json.as_bytes())?;
    writer.flush()
}
