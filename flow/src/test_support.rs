//! Helpers shared by the unit tests: a subscriber configuration from a YAML
//! fragment, free ports, temporary paths, a ZeroMQ receive that waits for the
//! subscription to take effect, and an HTTP server that records requests.

use crate::config::{parse, SubscriberConfig, SubscriberConfigs};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A subscriber named `Example` reading every topic of the local feed, with
/// `body` (YAML lines indented four spaces) appended.
pub(crate) fn subscriber(body: &str) -> SubscriberConfig {
    let yaml = format!(
        "subscribers:\n  - name: Example\n    zmq_url: \"127.0.0.1\"\n    zmq_port: 12000\n    topics: [\"*\"]\n{body}"
    );
    parse::<SubscriberConfigs>(&yaml, "test")
        .unwrap()
        .subscribers
        .remove(0)
}

/// A loopback TCP port that was free a moment ago.
pub(crate) fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A path in the temporary directory that no other test or process uses.
pub(crate) fn temp_path(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "flow-test-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Call `publish` until a message arrives on `subscriber`. A PUB socket drops
/// what it sends before the subscription reaches it.
pub(crate) fn receive(subscriber: &zmq::Socket, mut publish: impl FnMut()) -> String {
    subscriber.set_rcvtimeo(100).unwrap();
    for _ in 0..100 {
        publish();
        if let Ok(Ok(message)) = subscriber.recv_string(0) {
            return message;
        }
    }
    panic!("no message arrived within ten seconds");
}

/// One request the stub received.
#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Request {
    /// The value of a header, by lower-case name.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// An HTTP/1.1 server on a loopback port that answers a fixed list of status
/// codes, one per request, and keeps what it was sent.
pub(crate) struct HttpStub {
    port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl HttpStub {
    pub(crate) fn answering(statuses: &[u16]) -> Self {
        Self::serving(statuses, None, Vec::new())
    }

    /// As [`HttpStub::answering`], with a `Location` header on every answer.
    pub(crate) fn redirecting(statuses: &[u16], location: &str) -> Self {
        Self::serving(statuses, Some(location.to_string()), Vec::new())
    }

    /// One answer with `status` and `body`.
    pub(crate) fn answering_with_body(status: u16, body: Vec<u8>) -> Self {
        Self::serving(&[status], None, body)
    }

    fn serving(statuses: &[u16], location: Option<String>, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let statuses = statuses.to_vec();
        thread::spawn(move || {
            for status in statuses {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                serve(stream, status, location.as_deref(), &body, &seen);
            }
        });
        HttpStub { port, requests }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

/// Read one request, record it, answer it with `status` and close the
/// connection. The request is recorded before the answer goes out, so a
/// client that has its answer can already see its request in the list.
fn serve(
    stream: TcpStream,
    status: u16,
    location: Option<&str>,
    answer: &[u8],
    seen: &Mutex<Vec<Request>>,
) -> Option<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    if header("expect").is_some_and(|value| value.eq_ignore_ascii_case("100-continue")) {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .ok()?;
    }
    let mut body = Vec::new();
    if let Some(length) = header("content-length").and_then(|value| value.parse().ok()) {
        body.resize(length, 0);
        reader.read_exact(&mut body).ok()?;
    } else if header("transfer-encoding").is_some_and(|value| value.contains("chunked")) {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).ok()?;
            let size = usize::from_str_radix(size.trim().split(';').next()?, 16).ok()?;
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).ok()?;
            if size == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..size]);
        }
    }

    seen.lock().unwrap().push(Request {
        method,
        path,
        headers,
        body,
    });

    // The storage SDKs read these headers from every answer; other clients
    // ignore them.
    let reason = if status < 400 { "OK" } else { "Error" };
    let location = location.map_or_else(String::new, |to| format!("Location: {to}\r\n"));
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         {location}\
         ETag: \"0x1\"\r\n\
         Last-Modified: Wed, 04 Mar 2026 12:00:00 GMT\r\n\
         Date: Wed, 04 Mar 2026 12:00:00 GMT\r\n\
         x-ms-request-id: 00000000-0000-0000-0000-000000000001\r\n\
         x-ms-request-server-encrypted: true\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n",
        answer.len()
    );
    reader.get_mut().write_all(response.as_bytes()).ok()?;
    // A client that has read all it wants may close before the end.
    let _ = reader.get_mut().write_all(answer);
    Some(())
}
