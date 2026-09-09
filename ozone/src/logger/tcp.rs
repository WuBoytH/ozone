use std::{io::Write, net::TcpListener, sync::mpsc::{self, Sender}, thread::{self, sleep, yield_now}, time::Duration};

use log::{error, Level, Metadata, Record};
use skyline::{libc::memalign, nn};

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
