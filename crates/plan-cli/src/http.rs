//! A minimal HTTP/1.1 server for `plan serve`: one thread per connection reads the request, the caller handles requests one at a time.
//!
//! It replaced tiny_http 0.12 (#46): its pool of 4 reader threads queued new connections while all 4 waited, without a timeout,
//! on connections the browser had opened ahead of time and never used, so a page load could hang.
//! Here every connection has its own reader with a read timeout, and every response closes the connection.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// How long a connection may stay silent (before and inside a request)
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest request body accepted (a whole plan for /api/import)
const MAX_BODY: usize = 32 << 20;

pub struct Request {
    pub method: String,
    /// Path and query, as sent
    pub url: String,
    pub body: String,
    stream: TcpStream,
}

impl Request {
    /// Sends 200 with this content type and body, then closes the connection
    pub fn respond(mut self, ctype: &str, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = self.stream.write_all(head.as_bytes()).and_then(|_| self.stream.write_all(body)).and_then(|_| self.stream.flush());
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

pub struct Server {
    listener: TcpListener,
}

impl Server {
    pub fn bind(addr: impl std::net::ToSocketAddrs) -> std::io::Result<Server> { Ok(Server { listener: TcpListener::bind(addr)? }) }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> { self.listener.local_addr() }

    /// Requests in the order they were read. Connections that send nothing or a malformed request are dropped
    pub fn incoming(self) -> mpsc::Receiver<Request> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in self.listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    if let Some(req) = read_request(stream) { let _ = tx.send(req); }
                });
            }
        });
        rx
    }
}

fn read_request(stream: TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, url) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 { return None; }
        let h = h.trim_end();
        if h.is_empty() { break; }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") { len = v.trim().parse().ok()?; }
        }
    }
    if len > MAX_BODY { return None; }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    Some(Request { method, url, body: String::from_utf8_lossy(&body).into_owned(), stream })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(addr: SocketAddr, raw: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn idle_connections_do_not_block_requests() {
        let server = Server::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let rx = server.incoming();
        std::thread::spawn(move || for req in rx { let b = format!("{} {} {}", req.method, req.url, req.body); req.respond("text/plain", b.as_bytes()) });
        // More silent connections than tiny_http had reader threads (4)
        let idle: Vec<TcpStream> = (0..8).map(|_| TcpStream::connect(addr).unwrap()).collect();
        let out = get(addr, "POST /api/x?a=1 HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\n\r\nhello");
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out}");
        assert!(out.contains("Connection: close\r\n"));
        assert!(out.ends_with("\r\n\r\nPOST /api/x?a=1 hello"), "{out}");
        drop(idle);
    }
}
