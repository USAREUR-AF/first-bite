//! `mind` does not exit before its sinks have delivered: a replay waits for a
//! slow sink, a sink that cannot deliver is counted and changes the exit
//! status, and a live capture that is told to stop behaves the same.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A directory for one test's configuration files.
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "mind-drain-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// An HTTP listener that is bound at once and begins to answer after `delay`,
/// as an endpoint whose first connection is slow would. It answers 200 to
/// every POST and keeps the bodies.
fn slow_endpoint(delay: Duration) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    thread::spawn(move || {
        thread::sleep(delay);
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let seen = Arc::clone(&seen);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                loop {
                    let mut length = 0;
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 {
                            return;
                        }
                        let header = header.trim_end().to_ascii_lowercase();
                        if header.is_empty() {
                            break;
                        }
                        if let Some(value) = header.strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; length];
                    if reader.read_exact(&mut body).is_err() {
                        return;
                    }
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&body).into_owned());
                    let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                    if reader.get_mut().write_all(answer).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, bodies)
}

/// Configuration files for one publisher on a port of its own and the given
/// subscriber; returns the environment `mind` is started with.
fn configuration(name: &str, subscriber: &str) -> Vec<(&'static str, String)> {
    let directory = scratch(name);
    let record_port = free_port();
    let publishers = directory.join("publishers.yml");
    std::fs::write(
        &publishers,
        format!(
            "publishers:\n  - name: records\n    publisher_type: PUB\n    address: \"tcp://127.0.0.1:{record_port}\"\n    topic: \"subsystem/message_type\"\n"
        ),
    )
    .unwrap();
    let subscribers = directory.join("subscribers.yml");
    std::fs::write(
        &subscribers,
        format!(
            "subscribers:\n  - name: Sink\n    zmq_url: \"127.0.0.1\"\n    zmq_port: {record_port}\n    topics: [\"*\"]\n{subscriber}"
        ),
    )
    .unwrap();
    vec![
        ("PUBLISHER_CONFIG_PATH", publishers.display().to_string()),
        ("SUBSCRIBER_CONFIG_PATH", subscribers.display().to_string()),
        (
            "RUST_LOG",
            "warn,mind=info,flow=info,mind::run=warn".to_string(),
        ),
    ]
}

fn mind(environment: &[(&'static str, String)], arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mind"));
    command
        .args(arguments)
        .current_dir(root())
        .env_remove("SUBPUB_CONFIG_PATH")
        .env_remove("SECURITY_CONFIG_PATH")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        command.env(name, value);
    }
    command
}

fn replay(environment: &[(&'static str, String)], extra: &[&str]) -> Output {
    let mut arguments = vec![
        "--pcap-file",
        "examples/weather-station.pcap",
        "--dictionary",
        "dictionaries/weather-station",
    ];
    arguments.extend_from_slice(extra);
    mind(environment, &arguments).output().unwrap()
}

fn log(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The endpoint takes three seconds to answer its first connection. Every
/// record of the capture arrives, and the run says so and exits 0.
#[test]
fn a_replay_waits_for_a_slow_sink_and_every_record_arrives() {
    let (port, bodies) = slow_endpoint(Duration::from_secs(3));
    let environment = configuration(
        "slow",
        &format!("    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: {port}\n    endpoint_path: /records\n"),
    );
    let started = Instant::now();
    let output = replay(&environment, &[]);
    let log = log(&output);
    assert!(started.elapsed() >= Duration::from_secs(3), "{log}");
    assert_eq!(output.status.code(), Some(0), "{log}");
    assert!(
        log.contains("sink Sink: 5 delivered, 0 failed, 0 abandoned"),
        "{log}"
    );
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 5);
    let types: Vec<String> = bodies
        .iter()
        .map(|body| {
            let record: serde_json::Value = serde_json::from_str(body).unwrap();
            record["message_type"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(
        types,
        ["Heartbeat", "Observation", "Observation", "Position", "42"]
    );
}

/// Nothing listens at the destination. The run reads every frame, reports
/// what the sink could not deliver, and exits 3.
#[test]
fn a_destination_that_is_away_is_counted_and_changes_the_exit_status() {
    let dead = free_port();
    let environment = configuration(
        "dead",
        &format!(
            "    subscriber_type: tcp\n    destinations:\n      - address: \"127.0.0.1:{dead}\"\n"
        ),
    );
    let started = Instant::now();
    let output = replay(&environment, &["--drain-seconds", "2"]);
    let log = log(&output);
    assert_eq!(output.status.code(), Some(3), "{log}");
    assert!(
        log.contains("sink Sink: 0 delivered, 0 failed, 5 abandoned"),
        "{log}"
    );
    assert!(
        log.contains("done: 5 frames, 5 records, 5 decoded"),
        "{log}"
    );
    // The wait is the one that was asked for, not longer.
    assert!(started.elapsed() < Duration::from_secs(10), "{log}");
}

/// An endpoint that refuses every record: each is tried and fails, none is
/// abandoned, and because nothing was delivered the exit status is 3.
#[test]
fn a_sink_that_delivers_nothing_changes_the_exit_status() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let environment = configuration(
        "refused",
        &format!("    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: {port}\n"),
    );
    let output = replay(&environment, &[]);
    let log = log(&output);
    assert_eq!(output.status.code(), Some(3), "{log}");
    assert!(
        log.contains("sink Sink: 0 delivered, 5 failed, 0 abandoned"),
        "{log}"
    );
}

/// With no sink configured a replay exits 0, as before.
#[test]
fn a_replay_with_no_sink_exits_zero() {
    let mut environment = configuration("none", "    subscriber_type: stateful\n");
    environment.retain(|(name, _)| *name != "SUBSCRIBER_CONFIG_PATH");
    let output = replay(&environment, &[]);
    assert_eq!(output.status.code(), Some(0), "{}", log(&output));
}

/// A live source, stopped with SIGTERM while the sink is still slow: the
/// records already published are delivered before the process exits.
#[cfg(unix)]
#[test]
fn a_stop_signal_drains_the_sinks_before_the_process_exits() {
    use etherparse::PacketBuilder;

    let (port, bodies) = slow_endpoint(Duration::from_secs(3));
    let environment = configuration(
        "live",
        &format!("    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: {port}\n"),
    );
    let mock_port = {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.local_addr().unwrap().port()
    };
    let mock_address = format!("127.0.0.1:{mock_port}");
    let child = mind(
        &environment,
        &[
            "--mock",
            "--mock-address",
            &mock_address,
            "--dictionary",
            "dictionaries/weather-station",
        ],
    )
    .spawn()
    .unwrap();

    // Frames are sent until the process has had time to bind and subscribe;
    // the mock source takes one Ethernet frame per datagram.
    let heartbeat = [
        0x01, 0x00, 0x0c, 0x4b, 0x58, 0x37, 0x00, 0x00, 0x01, 0x51, 0x80, 0x60,
    ];
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
        .udp(50000, 40100);
    let mut frame = Vec::new();
    builder.write(&mut frame, &heartbeat).unwrap();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    thread::sleep(Duration::from_millis(1500));
    for _ in 0..7 {
        sender.send_to(&frame, &mock_address).unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(300));
    // The endpoint has not answered yet: nothing is delivered so far.
    assert!(bodies.lock().unwrap().is_empty());
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let output = child.wait_with_output().unwrap();
    let log = log(&output);
    assert_eq!(output.status.code(), Some(0), "{log}");
    assert!(
        log.contains("sink Sink: 7 delivered, 0 failed, 0 abandoned"),
        "{log}"
    );
    assert_eq!(bodies.lock().unwrap().len(), 7);
}
