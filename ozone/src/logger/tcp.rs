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
//!
//! Sockets go through the nn::socket C API ([`Socket`]), not `std::net`. On this target std
//! builds a failed call's error from its return value, which nnSdk sets to -1, so every error
//! reads "os error 1" and a non-blocking accept never reports `WouldBlock`. std also closes
//! with libc `close`, which nnSdk ignores for socket descriptors. Together that leaked one
//! socket per rebind until the process ran out of them.

use std::{
    collections::VecDeque,
    ffi::c_void,
    mem::size_of,
    net::{Ipv4Addr, SocketAddrV4},
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
const BACKLOG_LIMIT: usize = 1 * 1024 * 1024;
/// How long one write may block before the client is considered gone. Also bounds how long the
/// `nn::socket::Finalize` hook may have to wait for the writer thread (best effort: ignored if
/// the socket layer rejects `SO_SNDTIMEO`).
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often the accept thread polls the (non-blocking) listener.
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Consecutive `accept` failures before the listening socket is closed and bound again.
const ACCEPT_FAILURES_BEFORE_REBIND: u32 = 5;
/// Stack size of the accept and writer threads. std's default on this target is 2 MiB per
/// thread, taken from the game's heap; these threads only make socket calls and format a few
/// short diagnostics.
const THREAD_STACK_SIZE: usize = 64 * 1024;

/// An errno from nn::socket.
type Errno = i32;
const EAGAIN: Errno = 11;

const AF_INET: i32 = 2;
const SOCK_STREAM: i32 = 1;
const SOL_SOCKET: i32 = 0xffff;
const SO_REUSEADDR: i32 = 0x4;
const SO_SNDTIMEO: i32 = 0x1005;
const IPPROTO_TCP: i32 = 6;
const TCP_NODELAY: i32 = 1;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 0x800;

#[repr(C)]
#[derive(Default)]
struct SockAddrIn {
    len: u8,
    family: u8,
    /// Network byte order.
    port: u16,
    addr: [u8; 4],
    zero: [u8; 8],
}

#[repr(C)]
struct TimeVal {
    sec: i64,
    usec: i64,
}

extern "C" {
    fn nnsocketSocket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn nnsocketBind(fd: i32, addr: *const SockAddrIn, len: u32) -> i32;
    fn nnsocketListen(fd: i32, backlog: i32) -> i32;
    fn nnsocketAccept(fd: i32, addr: *mut SockAddrIn, len: *mut u32) -> i32;
    fn nnsocketFcntl(fd: i32, cmd: i32, ...) -> i32;
    fn nnsocketSetSockOpt(fd: i32, level: i32, name: i32, value: *const c_void, len: u32) -> i32;
    fn nnsocketSend(fd: i32, buffer: *const c_void, len: usize, flags: i32) -> isize;
    fn nnsocketClose(fd: i32) -> i32;
    fn nnsocketGetLastErrno() -> Errno;
}

/// Maps nn::socket's -1 to the calling thread's errno.
fn check(ret: i32) -> Result<i32, Errno> {
    if ret < 0 {
        Err(unsafe { nnsocketGetLastErrno() })
    } else {
        Ok(ret)
    }
}

/// A socket descriptor, closed through nn::socket when dropped.
struct Socket(i32);

impl Socket {
    /// A TCP socket listening on `port` on every interface.
    fn listen(port: u16) -> Result<Socket, Errno> {
        let socket = Socket(check(unsafe { nnsocketSocket(AF_INET, SOCK_STREAM, 0) })?);
        // A client connection left in TIME_WAIT must not hold the port against the next bind.
        socket.set_opt(SOL_SOCKET, SO_REUSEADDR, &1i32)?;
        let addr = SockAddrIn {
            len: size_of::<SockAddrIn>() as u8,
            family: AF_INET as u8,
            port: port.to_be(),
            ..Default::default()
        };
        check(unsafe { nnsocketBind(socket.0, &addr, size_of::<SockAddrIn>() as u32) })?;
        check(unsafe { nnsocketListen(socket.0, 128) })?;
        Ok(socket)
    }

    fn accept(&self) -> Result<(Socket, SocketAddrV4), Errno> {
        let mut addr = SockAddrIn::default();
        let mut len = size_of::<SockAddrIn>() as u32;
        let fd = check(unsafe { nnsocketAccept(self.0, &mut addr, &mut len) })?;
        Ok((Socket(fd), SocketAddrV4::new(Ipv4Addr::from(addr.addr), u16::from_be(addr.port))))
    }

    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), Errno> {
        let flags = check(unsafe { nnsocketFcntl(self.0, F_GETFL) })?;
        let flags = if nonblocking { flags | O_NONBLOCK } else { flags & !O_NONBLOCK };
        check(unsafe { nnsocketFcntl(self.0, F_SETFL, flags) }).map(drop)
    }

    fn set_opt<T>(&self, level: i32, name: i32, value: &T) -> Result<(), Errno> {
        let value = value as *const T as *const c_void;
        check(unsafe { nnsocketSetSockOpt(self.0, level, name, value, size_of::<T>() as u32) }).map(drop)
    }

    fn send_all(&self, mut bytes: &[u8]) -> Result<(), Errno> {
        while !bytes.is_empty() {
            let sent = unsafe { nnsocketSend(self.0, bytes.as_ptr() as *const c_void, bytes.len(), 0) };
            if sent < 0 {
                return Err(unsafe { nnsocketGetLastErrno() });
            }
            bytes = &bytes[sent as usize..];
        }
        Ok(())
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { nnsocketClose(self.0) };
    }
}

struct Client {
    /// False between `nn::socket::Finalize` and the next `nn::socket::Initialize`: no socket
    /// call may be made.
    enabled: bool,
    listener: Option<Socket>,
    stream: Option<Socket>,
    accept_failures: u32,
    bind_failure_reported: bool,
    backlog: VecDeque<String>,
    backlog_bytes: usize,
}

static CLIENT: Mutex<Client> = Mutex::new(Client {
    enabled: true,
    listener: None,
    stream: None,
    accept_failures: 0,
    bind_failure_reported: false,
    backlog: VecDeque::new(),
    backlog_bytes: 0,
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
        match Socket::listen(PORT) {
            Ok(listener) => {
                if let Err(err) = listener.set_nonblocking(true) {
                    // A blocking accept would hold the lock forever; better no logger.
                    debug(&format!("[ozone] TCP log listener cannot be made non-blocking, logger disabled: errno {}", err));
                    self.enabled = false;
                    return;
                }
                self.accept_failures = 0;
                self.bind_failure_reported = false;
                self.listener = Some(listener);
            },
            Err(err) => {
                if !self.bind_failure_reported {
                    debug(&format!("[ozone] Could not bind the TCP log port {}: errno {}", PORT, err));
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
                let _ = stream.set_opt(IPPROTO_TCP, TCP_NODELAY, &1i32);
                let timeout = TimeVal { sec: WRITE_TIMEOUT.as_secs() as i64, usec: WRITE_TIMEOUT.subsec_micros() as i64 };
                let _ = stream.set_opt(SOL_SOCKET, SO_SNDTIMEO, &timeout);
                debug(&format!("[ozone] TCP log client connected from {}", peer));
                self.connect(stream);
            },
            Err(EAGAIN) => {},
            Err(err) => {
                // Seen when the network goes away (sleep, connection loss): the socket layer may
                // keep failing, so recreate the listener after a few attempts.
                self.accept_failures += 1;
                debug(&format!("[ozone] TCP log accept failed ({}/{}): errno {}", self.accept_failures, ACCEPT_FAILURES_BEFORE_REBIND, err));
                if self.accept_failures >= ACCEPT_FAILURES_BEFORE_REBIND {
                    self.listener = None;
                    self.accept_failures = 0;
                }
            },
        }
    }

    /// Sends `message` to the connected client, or keeps it for the next one.
    fn write(&mut self, message: String) {
        if let Some(stream) = self.stream.as_ref() {
            match stream.send_all(message.as_bytes()) {
                Ok(()) => return,
                Err(err) => {
                    debug(&format!("[ozone] TCP log client lost: errno {}", err));
                    self.stream = None;
                },
            }
        }
        self.push_backlog(message);
    }

    fn push_backlog(&mut self, message: String) {
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
    fn connect(&mut self, stream: Socket) {
        while let Some(message) = self.backlog.pop_front() {
            self.backlog_bytes -= message.len();
            if let Err(err) = stream.send_all(message.as_bytes()) {
                debug(&format!("[ozone] TCP log client lost while replaying backlog: errno {}", err));
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
        let Some(stream) = client.stream.as_ref() else { return false };
        match stream.send_all(message.as_bytes()) {
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
    let builder = thread::Builder::new().name(name.into()).stack_size(THREAD_STACK_SIZE);
    if let Err(err) = builder.spawn(body) {
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
    let pool = unsafe { memalign(0x1000, 0x600000) as *mut u8 };
    let rc = unsafe { nn::socket::Initialize(pool, 0x600000, 0x20000, 14) };
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
