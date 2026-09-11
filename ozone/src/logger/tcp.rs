//! TCP log sink on port 6969.
//!
//! Two threads: an accept thread that polls a non-blocking listener, and a writer thread that
//! drains the log channel into the connected client. While no client is connected the most
//! recent output is kept in a bounded backlog and replayed on connect.
//!
//! Every socket call made here happens under [`CLIENT`]'s lock and is non-blocking or bounded
//! by a timeout, so the `nn::socket::Finalize` hook in `lib.rs` can take the lock, close our
//! sockets and let the game finalize the socket library without any of our IPCs in flight
//! (nnSdk asserts inside e.g. `nn::socket::detail::Accept` if a call is pending when the
//! library goes away. [`TcpLogger::suspend`] / [`TcpLogger::resume` are the hooks' entry
//! points.
//!
//! Nothing in this module may panic or exit on an I/O error. Every `println!` in ozone and in
//! every plugin ends up in [`log::Log::log`] below (std's stdout on this target is
//! `skyline_tcp_send_raw`), and so do the panic hook and the crash handler. A panic here
//! panics again while the panic is being printed, without bound; a dead channel turns every
//! log line in the process into such a panic.

use std::{
    collections::VecDeque,
    io::{ErrorKind, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex, MutexGuard,
    },
    thread,
    time::Duration,
};

use log::{Level, Metadata, Record};
use skyline::{libc::memalign, nn};

const PORT: u16 = 6969;
/// Most bytes of log output kept while no client is connected.
const BACKLOG_LIMIT: usize = 2 * 1024 * 1024;
/// How long one write may block before the client is considered gone. Also bounds how long the
/// `nn::socket::Finalize` hook may have to wait for the writer thread (best effort: ignored if
/// the socket layer rejects `SO_SNDTIMEO`).
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often the accept thread polls the (non-blocking) listener.
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Consecutive `accept` failures before the listening socket is closed and bound again.
const ACCEPT_FAILURES_BEFORE_REBIND: u32 = 5;

struct Client {
    /// False between `nn::socket::Finalize` and the next `nn::socket::Initialize`: no socket
    /// call may be made.
    enabled: bool,
    listener: Option<TcpListener>,
    stream: Option<TcpStream>,
    accept_failures: u32,
    bind_failure_reported: bool,
    backlog: VecDeque<String>,
    backlog_bytes: usize,
    had_client: bool
}

static CLIENT: Mutex<Client> = Mutex::new(Client {
    enabled: true,
    listener: None,
    stream: None,
    accept_failures: 0,
    bind_failure_reported: false,
    backlog: VecDeque::new(),
    backlog_bytes: 0,
    had_client: false
});

impl Client {
    fn lock() -> MutexGuard<'static, Client> {
        CLIENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Creates the listening socket if there is none. Failures are retried on the next poll.
    fn ensure_listener(&mut self) {
        if self.listener.is_some() {
            return;
        }
        match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], PORT))) {
            Ok(listener) => {
                if let Err(err) = listener.set_nonblocking(true) {
                    // A blocking accept would hold the lock forever; better no logger.
                    debug(&format!("[ozone] TCP log listener cannot be made non-blocking, logger disabled: {}", err));
                    self.enabled = false;
                    return;
                }
                self.accept_failures = 0;
                self.bind_failure_reported = false;
                self.listener = Some(listener);
            },
            Err(err) => {
                if !self.bind_failure_reported {
                    debug(&format!("[ozone] Could not bind the TCP log port {}: {}", PORT, err));
                    self.bind_failure_reported = true;
                }
            },
        }
    }

    /// One non-blocking accept attempt.
    fn poll_accept(&mut self) {
        let Some(listener) = self.listener.as_ref() else { return };
        match listener.accept() {
            Ok((stream, peer)) => {
                self.accept_failures = 0;
                // Accepted sockets may inherit the listener's non-blocking mode on BSD.
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_nodelay(true);
                let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                debug(&format!("[ozone] TCP log client connected from {}", peer));
                self.connect(stream);
            },
            Err(err) if err.kind() == ErrorKind::WouldBlock => {},
            Err(err) => {
                // Seen when the network goes away (sleep, connection loss): the socket layer may
                // keep failing, so recreate the listener after a few attempts.
                self.accept_failures += 1;
                debug(&format!("[ozone] TCP log accept failed ({}/{}): {}", self.accept_failures, ACCEPT_FAILURES_BEFORE_REBIND, err));
                if self.accept_failures >= ACCEPT_FAILURES_BEFORE_REBIND {
                    self.listener = None;
                    self.accept_failures = 0;
                }
            },
        }
    }

    /// Sends `message` to the connected client, or keeps it for the next one.
    fn write(&mut self, message: String) {
        if let Some(stream) = self.stream.as_mut() {
            match stream.write_all(message.as_bytes()) {
                Ok(()) => return,
                Err(err) => {
                    debug(&format!("[ozone] TCP log client lost: {}", err));
                    self.stream = None;
                },
            }
        }
        self.push_backlog(message);
    }

    fn push_backlog(&mut self, message: String) {
        if !self.had_client {
            return;
        }
        if message.len() > BACKLOG_LIMIT {
            return;
        }
        while self.backlog_bytes + message.len() > BACKLOG_LIMIT {
            match self.backlog.pop_front() {
                Some(dropped) => self.backlog_bytes -= dropped.len(),
                None => break,
            }
        }
        self.backlog_bytes += message.len();
        self.backlog.push_back(message);
    }

    /// Replays the backlog to `stream` and makes it the current client. A previous client, if
    /// any, is dropped (and so closed).
    fn connect(&mut self, mut stream: TcpStream) {
        self.had_client = true;
        while let Some(message) = self.backlog.pop_front() {
            self.backlog_bytes -= message.len();
            if let Err(err) = stream.write_all(message.as_bytes()) {
                debug(&format!("[ozone] TCP log client lost while replaying backlog: {}", err));
                // Keep it for the next client.
                self.backlog_bytes += message.len();
                self.backlog.push_front(message);
                return;
            }
        }
        self.stream = Some(stream);
    }
}

pub struct TcpLogger(Sender<String>);

impl TcpLogger {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<String>();

        spawn("tcp-logger-accept", accept_loop);
        spawn("tcp-logger-writer", move || writer_loop(receiver));

        TcpLogger(sender)
    }

    /// Closes the logger's sockets and stops it from making socket calls. Called by the
    /// `nn::socket::Finalize` hook right before the game finalizes the socket library; returns
    /// once no logger IPC is in flight (waits for a write in progress, at most `WRITE_TIMEOUT`).
    pub fn suspend() {
        let mut client = Client::lock();
        client.enabled = false;
        client.stream = None;
        client.listener = None;
    }

    /// Lets the logger use sockets again; the accept thread rebinds on its next poll and the
    /// backlog is replayed to the next client. Called after `nn::socket::Initialize` succeeded.
    pub fn resume() {
        let mut client = Client::lock();
        client.enabled = true;
        client.bind_failure_reported = false;
    }

    /// Writes `message` straight to the connected client on the calling thread, for the crash
    /// handler. Returns false if no client is connected, the client is busy (the writer or
    /// accept thread holds it), or the write failed.
    pub fn write_direct(message: &str) -> bool {
        let Ok(mut client) = CLIENT.try_lock() else { return false };
        if !client.enabled {
            return false;
        }
        let Some(stream) = client.stream.as_mut() else { return false };
        match stream.write_all(message.as_bytes()).and_then(|_| stream.flush()) {
            Ok(()) => true,
            Err(_) => {
                client.stream = None;
                false
            },
        }
    }
}

impl log::Log for TcpLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Fails only if the writer thread is gone, and then there is nothing left to do.
            let _ = self.0.send(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) {
    if let Err(err) = thread::Builder::new().name(name.into()).spawn(body) {
        debug(&format!("[ozone] Could not start the {} thread: {}", name, err));
    }
}

fn writer_loop(receiver: Receiver<String>) {
    // `recv` fails only once every sender is gone, which never happens for a static logger.
    while let Ok(message) = receiver.recv() {
        let mut client = Client::lock();
        if client.enabled {
            client.write(message);
        } else {
            client.push_backlog(message);
        }
    }
}

fn accept_loop() {
    // Goes through `socket_initialize_hook`: performs the real initialization unless the game
    // got there first, in which case it is a no-op and the game's configuration is in force.
    let pool = unsafe { memalign(0x1000, 0x100000) as *mut u8 };
    let rc = unsafe { nn::socket::Initialize(pool, 0x100000, 0x20000, 14) };
    if rc != 0 {
        debug(&format!("[ozone] nn::socket::Initialize returned {:#x}", rc));
    }

    loop {
        {
            let mut client = Client::lock();
            if client.enabled {
                client.ensure_listener();
                client.poll_accept();
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Kernel debug log; the one sink that does not depend on this module working.
fn debug(message: &str) {
    let _ = horizon_svc::output_debug_string(message);
}
