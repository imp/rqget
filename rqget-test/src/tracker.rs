//! Minimal loopback tracker fixture; all returned peers are controlled by the test.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, ensure};

#[derive(Debug)]
pub struct Tracker {
    pub url: url::Url,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Tracker {
    /// Start a loopback tracker returning the supplied fixture peer.
    ///
    /// # Errors
    /// Returns an error for non-loopback/non-IPv4 peers or a listener setup failure.
    pub fn start(peer: SocketAddr) -> Result<Self> {
        let SocketAddr::V4(peer) = peer else {
            anyhow::bail!("fixture requires IPv4");
        };
        ensure!(peer.ip().is_loopback(), "fixture peer must be loopback");
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let url = url::Url::parse(&format!("http://{}/announce", listener.local_addr()?))?;
        let mut body = b"d8:intervali60e5:peers6:".to_vec();
        body.extend_from_slice(&peer.ip().octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let mut request = [0; 8192];
                        if stream.read(&mut request).is_ok() {
                            let _ = stream.write_all(&response);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::park_timeout(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            url,
            stopped,
            worker: Some(worker),
        })
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
