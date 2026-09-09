use std::{io::Write, net::{TcpListener, TcpStream}, sync::{mpsc::{self, Sender}, Mutex}, thread::{self, sleep, yield_now}, time::Duration};

use log::{error, Level, Metadata, Record};
use skyline::{libc::memalign, nn};

/// A clone of the connected client's socket, so the crash handler can write a report
/// synchronously instead of queueing it for the logger thread (which may never get to
/// run before the process is torn down).
static DIRECT_STREAM: Mutex<Option<TcpStream>> = Mutex::new(None);

pub struct TcpLogger(Sender<String>);

impl TcpLogger {
    pub fn new() -> Self {

        let (sender, receiver) = mpsc::channel::<String>();

        std::thread::Builder::new()
            .name("tcp-logger".into())
            .spawn(move || {
                let pool = unsafe { memalign(0x1000, 0x100000) as *mut u8 };

                unsafe { nn::socket::Initialize(pool, 0x100000, 0x20000, 14) };

                let listener = TcpListener::bind("0.0.0.0:6969").unwrap();

                if let Some(Ok(mut stream)) = listener.incoming().next() {
                    *DIRECT_STREAM.lock().unwrap() = stream.try_clone().ok();

                    thread::spawn(move || {
                        loop {
                            match receiver.recv() {
                                Ok(message) => { stream.write(message.as_bytes()).unwrap(); },
                                Err(err) => {
                                    panic!("Listener thread ran into an error: {}", err);
                                },
                            }
                        }
                    });
                }
        }).unwrap();

        TcpLogger(sender)
    }

    /// Writes `message` straight to the connected client on the calling thread.
    /// Returns false if no client is connected, the socket is in use, or the write failed.
    pub fn write_direct(message: &str) -> bool {
        let Ok(guard) = DIRECT_STREAM.try_lock() else { return false };
        let Some(stream) = guard.as_ref() else { return false };
        let mut stream: &TcpStream = stream;
        stream.write_all(message.as_bytes()).is_ok() && stream.flush().is_ok()
    }
}

impl log::Log for TcpLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            self.0.send(record.args().to_string()).unwrap();
        }
    }

    fn flush(&self) {}
}
