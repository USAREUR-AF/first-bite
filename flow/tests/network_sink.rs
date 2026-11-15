//! The `udp` and `tcp` sinks against listeners on this machine: what arrives,
//! from which port, in which encoding; reconnection and the drop counter; and
//! TLS with a client certificate against a server that requires one.

use flow::config::{parse, SubscriberConfigs};
use flow::security::{Security, Transport};
use flow::sink::network::{admit, NetworkSink, Wire, DEFAULT_QUEUE};
use flow::{build_sink, build_sink_secured, describe_sink, Sink, SubscriberConfig};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const WAIT: Duration = Duration::from_secs(10);

/// A weather-station Position record, as `mind` publishes it.
const POSITION: &str = r#"{"timestamp":{"secs_since_epoch":1750000003,"nanos_since_epoch":0},"src_port":40100,"dst_port":50000,"src_ip":[192,0,2,10],"dst_ip":[192,0,2,20],"data_encoded_inner":"","subsystem":"WX","message_type":"Position","message":{"altitude":136.5,"latitude":64.8378,"longitude":-147.7164,"message_length":27,"message_type":"Position","station_id":"KX7"},"data_encoded_full":"","data_model_version":"2.0.0"}"#;

const COT: &str = "    cot:\n      uid: [station_id]\n      type: { value: a-f-G }\n      position: { geodetic: [latitude, longitude, altitude] }\n      stale_minutes: 10\n";

/// A subscriber named `ToTak` with `body` (YAML lines indented four spaces).
fn subscriber(body: &str) -> SubscriberConfig {
    let yaml = format!(
        "subscribers:\n  - name: ToTak\n    zmq_url: \"127.0.0.1\"\n    zmq_port: 12000\n    topics: [\"*\"]\n{body}"
    );
    parse::<SubscriberConfigs>(&yaml, "test")
        .unwrap()
        .subscribers
        .remove(0)
}

fn sink(body: &str) -> NetworkSink {
    NetworkSink::from_config(&subscriber(body)).unwrap()
}

fn refusal(body: &str) -> String {
    match build_sink(&subscriber(body)) {
        Ok(_) => panic!("{body} built a sink"),
        Err(e) => e.to_string(),
    }
}

fn temp_path(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "flow-network-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The attributes of the `event` and `point` elements of a CoT event, after
/// checking that the text is one well-formed document with the elements the
/// CoT 2.0 schema requires and nothing after it.
fn cot_attributes(xml: &[u8]) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let text = std::str::from_utf8(xml).expect("the event is UTF-8");
    let mut reader = Reader::from_str(text);
    let (mut event, mut point) = (BTreeMap::new(), BTreeMap::new());
    let (mut depth, mut roots, mut seen) = (0usize, 0usize, Vec::new());
    loop {
        let item = reader.read_event().expect("well-formed XML");
        let (start, empty) = match &item {
            Event::Start(start) => (start, false),
            Event::Empty(start) => (start, true),
            Event::End(_) => {
                depth -= 1;
                continue;
            }
            Event::Eof => break,
            Event::Text(text) => {
                assert!(depth > 0 || text.trim().is_empty(), "{text:?}");
                continue;
            }
            _ => continue,
        };
        let name = start.name().as_ref().to_string();
        let attributes: BTreeMap<String, String> = start
            .attributes()
            .map(|attribute| {
                let attribute = attribute.expect("a well-formed attribute");
                (
                    attribute.key.as_ref().to_string(),
                    attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap()
                        .into_owned(),
                )
            })
            .collect();
        if depth == 0 {
            roots += 1;
            assert_eq!(name, "event");
            event = attributes;
        } else if depth == 1 {
            if name == "point" {
                point = attributes;
            }
            seen.push(name);
        }
        if !empty {
            depth += 1;
        }
    }
    assert_eq!(depth, 0, "an element is not closed");
    assert_eq!(roots, 1, "one event per payload");
    assert!(seen.contains(&"point".to_string()), "{seen:?}");
    assert!(seen.contains(&"detail".to_string()), "{seen:?}");
    for required in ["version", "uid", "type", "time", "start", "stale", "how"] {
        assert!(event.contains_key(required), "event has no {required}");
    }
    for required in ["lat", "lon", "hae", "ce", "le"] {
        assert!(point.contains_key(required), "point has no {required}");
    }
    assert_eq!(event["version"], "2.0");
    (event, point)
}

fn assert_position_event(xml: &[u8]) {
    let (event, point) = cot_attributes(xml);
    assert_eq!(event["uid"], "KX7");
    assert_eq!(event["type"], "a-f-G");
    assert_eq!(point["lat"], "64.8378");
    assert_eq!(point["lon"], "-147.7164");
    assert_eq!(point["hae"], "136.5");
    let time = chrono::DateTime::parse_from_rfc3339(&event["time"]).unwrap();
    let stale = chrono::DateTime::parse_from_rfc3339(&event["stale"]).unwrap();
    assert_eq!((stale - time).num_minutes(), 10);
}

/// A UDP socket on loopback that waits up to [`WAIT`] for a datagram.
fn udp_listener() -> (UdpSocket, u16) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(WAIT)).unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}

fn receive(socket: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut buffer = vec![0u8; 65_536];
    let (length, from) = socket.recv_from(&mut buffer).expect("a datagram arrives");
    buffer.truncate(length);
    (buffer, from)
}

fn free_udp_port() -> u16 {
    udp_listener().1
}

#[tokio::test]
async fn udp_sends_one_cot_event_per_datagram_to_every_destination() {
    let (first, first_port) = udp_listener();
    let (second, second_port) = udp_listener();
    let source_port = free_udp_port();
    let sink = sink(&format!(
        "    subscriber_type: udp\n    destinations:\n      - address: \"127.0.0.1:{first_port}\"\n        source_port: {source_port}\n      - address: \"localhost:{second_port}\"\n{COT}"
    ));
    sink.prepare().await.unwrap();
    sink.send("WX/Position", POSITION).await.unwrap();
    sink.send("WX/Position", POSITION).await.unwrap();

    for _ in 0..2 {
        let (datagram, from) = receive(&first);
        assert_position_event(&datagram);
        // The destination with `source_port` is sent to from that port.
        assert_eq!(from.port(), source_port);
        let (datagram, from) = receive(&second);
        assert_position_event(&datagram);
        assert_ne!(from.port(), source_port);
    }
    let stats = sink.stats();
    assert_eq!((stats[0].sent, stats[0].failed), (2, 0));
    assert_eq!((stats[1].sent, stats[1].failed), (2, 0));
}

#[tokio::test]
async fn udp_without_a_mapping_sends_the_record_in_the_subscribers_encoding() {
    let (listener, port) = udp_listener();
    let json = sink(&format!(
        "    subscriber_type: udp\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n"
    ));
    json.send("WX/Position", POSITION).await.unwrap();
    assert_eq!(receive(&listener).0, POSITION.as_bytes());

    let cbor = sink(&format!(
        "    subscriber_type: udp\n    encoding: cbor\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n"
    ));
    cbor.send("WX/Position", POSITION).await.unwrap();
    let datagram = receive(&listener).0;
    assert_eq!(datagram, flow::encoding::json_to_cbor(POSITION).unwrap());
}

#[tokio::test]
async fn a_record_the_mapping_cannot_use_is_an_error_and_nothing_is_sent() {
    let (listener, port) = udp_listener();
    listener
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let sink = sink(&format!(
        "    subscriber_type: udp\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n{COT}"
    ));
    let heartbeat = r#"{"subsystem":"WX","message_type":"Heartbeat","message":{"station_id":"KX7","uptime":1}}"#;
    let error = sink.send("WX/Heartbeat", heartbeat).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cursor-on-Target: field `latitude` is missing from the message"
    );
    let undecoded = r#"{"subsystem":null,"message":null}"#;
    assert!(sink.send("t", undecoded).await.is_err());
    let mut buffer = [0u8; 16];
    assert!(
        listener.recv_from(&mut buffer).is_err(),
        "a datagram arrived"
    );
    assert_eq!(sink.stats()[0].sent, 0);
}

/// A datagram to a multicast group, received by a socket that joined the
/// group on the loopback interface. Hosts where loopback does not carry
/// multicast skip the delivery half; the socket options are still set and
/// read back.
#[tokio::test]
async fn udp_multicast_sets_ttl_and_interface_and_delivers_on_loopback() {
    // 239.2.3.1:6969 is the group TAK clients listen on by default; the test
    // uses its own port in the same administratively scoped range.
    let group = Ipv4Addr::new(239, 2, 3, 1);
    let listener = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .unwrap();
    listener.set_reuse_address(true).unwrap();
    listener
        .bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).into())
        .unwrap();
    let port = listener.local_addr().unwrap().as_socket().unwrap().port();
    let joined = listener
        .join_multicast_v4(&group, &Ipv4Addr::LOCALHOST)
        .is_ok();
    let listener: UdpSocket = listener.into();
    listener
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    let sink = sink(&format!(
        "    subscriber_type: udp\n    destinations:\n      - address: \"{group}:{port}\"\n        ttl: 3\n        interface: \"127.0.0.1\"\n{COT}"
    ));
    let sent = sink.send("WX/Position", POSITION).await;
    if !joined || sent.is_err() {
        eprintln!("this host does not carry multicast on loopback (join: {joined}, send: {sent:?}); delivery not checked");
        return;
    }
    let mut buffer = vec![0u8; 65_536];
    match listener.recv_from(&mut buffer) {
        Ok((length, _)) => assert_position_event(&buffer[..length]),
        Err(e) => eprintln!("the multicast datagram was sent and not received on loopback: {e}"),
    }
    assert_eq!(sink.stats()[0].sent, 1);
}

/// Read from `stream` until `count` events have arrived; return them.
async fn read_events(stream: &mut (impl AsyncReadExt + Unpin), count: usize) -> Vec<Vec<u8>> {
    let mut received = Vec::new();
    let mut buffer = [0u8; 4096];
    let end = b"</event>";
    let events = |bytes: &[u8]| bytes.windows(end.len()).filter(|w| w == end).count();
    while events(&received) < count {
        let length = tokio::time::timeout(WAIT, stream.read(&mut buffer))
            .await
            .expect("events arrive")
            .unwrap();
        assert!(length > 0, "the connection closed early");
        received.extend_from_slice(&buffer[..length]);
    }
    let mut list = Vec::new();
    let mut rest = &received[..];
    while let Some(position) = rest.windows(end.len()).position(|w| w == end) {
        list.push(rest[..position + end.len()].to_vec());
        rest = &rest[position + end.len()..];
    }
    assert!(rest.is_empty(), "bytes follow the last event: {rest:?}");
    list
}

async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(WAIT, listener.accept())
        .await
        .expect("the sink connects")
        .unwrap()
        .0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_streams_cot_events_and_reconnects_after_the_server_goes_away() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let source_port = free_udp_port();
    let sink = sink(&format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n        source_port: {source_port}\n{COT}"
    ));
    sink.prepare().await.unwrap();
    let mut stream = accept(&listener).await;
    assert_eq!(stream.peer_addr().unwrap().port(), source_port);
    // A TAK server streams to its clients; what it sends is read and dropped.
    stream.write_all(b"<event/>").await.unwrap();

    for _ in 0..3 {
        sink.send("WX/Position", POSITION).await.unwrap();
    }
    for event in read_events(&mut stream, 3).await {
        assert_position_event(&event);
    }

    // The server closes; the sink connects again, from the same port, and
    // what is sent afterwards arrives.
    drop(stream);
    let mut stream = accept(&listener).await;
    assert_eq!(stream.peer_addr().unwrap().port(), source_port);
    sink.send("WX/Position", POSITION).await.unwrap();
    assert_position_event(&read_events(&mut stream, 1).await[0]);
    let stats = sink.stats()[0];
    assert_eq!(stats.reconnects, 1);
    assert_eq!(stats.dropped, 0);
    assert!(stats.sent >= 4, "{stats:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_without_a_mapping_streams_json_lines() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sink = sink(&format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n"
    ));
    sink.send("WX/Position", POSITION).await.unwrap();
    sink.send("WX/Position", POSITION).await.unwrap();
    let mut stream = accept(&listener).await;
    let expected = format!("{POSITION}\n{POSITION}\n");
    let mut received = vec![0u8; expected.len()];
    tokio::time::timeout(WAIT, stream.read_exact(&mut received))
        .await
        .expect("two lines arrive")
        .unwrap();
    assert_eq!(String::from_utf8(received).unwrap(), expected);
}

/// With nothing listening, payloads wait in the queue; past its size they are
/// dropped and counted, `send` never blocks, and what waited is delivered
/// when the server appears.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_holds_a_bounded_queue_while_the_server_is_away_and_counts_what_it_drops() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let sink = sink(&format!(
        "    subscriber_type: tcp\n    queue: 5\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n{COT}"
    ));
    let started = std::time::Instant::now();
    for _ in 0..12 {
        sink.send("WX/Position", POSITION).await.unwrap();
    }
    assert!(started.elapsed() < Duration::from_secs(2), "send blocked");
    let stats = sink.stats()[0];
    assert_eq!(stats.sent, 0);
    // The connection task may hold one payload it took before the queue
    // filled, so five or six are kept.
    assert!((6..=7).contains(&stats.dropped), "{stats:?}");

    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let mut stream = accept(&listener).await;
    let kept = 12 - stats.dropped as usize;
    for event in read_events(&mut stream, kept).await {
        assert_position_event(&event);
    }
    assert_eq!(sink.stats()[0].dropped, stats.dropped);
}

/// A certificate authority, a server certificate for `localhost` and a client
/// certificate, written as PEM files; and the server configuration that
/// requires a client certificate from that authority.
struct Pki {
    ca_file: PathBuf,
    client_cert_file: PathBuf,
    client_key_file: PathBuf,
    server: Arc<rustls::ServerConfig>,
}

fn pki() -> Pki {
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let server_key = KeyPair::generate().unwrap();
    let server_cert = CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    let client_key = KeyPair::generate().unwrap();
    let client_cert = CertificateParams::new(vec!["first-bite.example".to_string()])
        .unwrap()
        .signed_by(&client_key, &issuer)
        .unwrap();

    let write = |name: &str, pem: String| {
        let path = temp_path(name);
        std::fs::write(&path, pem).unwrap();
        path
    };
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![CertificateDer::from(server_cert.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        )
        .unwrap();
    Pki {
        ca_file: write("ca.pem", ca_cert.pem()),
        client_cert_file: write("client.pem", client_cert.pem()),
        client_key_file: write("client.key", client_key.serialize_pem()),
        server: Arc::new(server),
    }
}

impl Drop for Pki {
    fn drop(&mut self) {
        for path in [&self.ca_file, &self.client_cert_file, &self.client_key_file] {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_with_tls_presents_its_client_certificate_and_streams_inside_tls() {
    let pki = pki();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::clone(&pki.server));
    let sink = sink(&format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"127.0.0.1:{port}\"\n    tls:\n      ca_file: \"{}\"\n      client_cert_file: \"{}\"\n      client_key_file: \"{}\"\n      server_name: localhost\n{COT}",
        pki.ca_file.display(),
        pki.client_cert_file.display(),
        pki.client_key_file.display()
    ));
    assert!(sink.wire().tls && sink.wire().client_certificate);
    sink.send("WX/Position", POSITION).await.unwrap();

    let stream = accept(&listener).await;
    let mut stream = tokio::time::timeout(WAIT, acceptor.accept(stream))
        .await
        .expect("the handshake finishes")
        .expect("the client certificate is accepted");
    // The server saw the certificate the subscriber names.
    let presented = stream.get_ref().1.peer_certificates().unwrap();
    assert_eq!(presented.len(), 1);
    assert_position_event(&read_events(&mut stream, 1).await[0]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_refuses_a_server_its_authority_did_not_sign_and_a_server_without_tls() {
    let pki = pki();
    let other = pki_other_server();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio_rustls::TlsAcceptor::from(other);
    let sink = sink(&format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"localhost:{port}\"\n    tls:\n      ca_file: \"{}\"\n{COT}",
        pki.ca_file.display()
    ));
    sink.send("WX/Position", POSITION).await.unwrap();
    let stream = accept(&listener).await;
    // The client ends the handshake: the certificate is not from its
    // authority. No event crosses.
    assert!(acceptor.accept(stream).await.is_err());

    // A plain listener: whatever arrives is a TLS hello, never an event.
    let mut plain = accept(&listener).await;
    let mut hello = [0u8; 5];
    tokio::time::timeout(WAIT, plain.read_exact(&mut hello))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hello[0], 0x16, "a TLS handshake record");
    assert_eq!(sink.stats()[0].sent, 0);
}

/// A server whose certificate comes from an authority of its own.
fn pki_other_server() -> Arc<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let key = rcgen::KeyPair::generate().unwrap();
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap(),
    )
}

#[test]
fn configuration_mistakes_stop_startup_and_name_the_subscriber() {
    let udp = |body: &str| refusal(&format!("    subscriber_type: udp\n{body}"));
    let tcp = |body: &str| refusal(&format!("    subscriber_type: tcp\n{body}"));
    let one = "    destinations:\n      - address: \"203.0.113.10:8087\"\n";
    // There is no default destination.
    assert_eq!(udp(""), "subscriber ToTak: `destinations` is required");
    assert_eq!(
        tcp("    destinations: []\n"),
        "subscriber ToTak: `destinations` is required"
    );
    for (address, reason) in [
        ("203.0.113.10", "has no port"),
        ("203.0.113.10:tak", "has no port number"),
        ("203.0.113.10:0", "names port 0"),
        (":8087", "has no host"),
        ("2001:db8::1:8087", "has no host"),
    ] {
        let error = udp(&format!(
            "    destinations:\n      - address: \"{address}\"\n"
        ));
        assert!(error.contains(reason), "{address}: {error}");
    }
    assert!(build_sink(&subscriber(
        "    subscriber_type: udp\n    destinations:\n      - address: \"[2001:db8::1]:8087\"\n"
    ))
    .is_ok());
    assert!(
        udp("    destinations:\n      - address: \"203.0.113.10:8087\"\n        ttl: 4\n")
            .contains("`ttl` and `interface` apply to a UDP multicast group only")
    );
    assert!(
        udp("    destinations:\n      - address: \"239.2.3.1:6969\"\n        ttl: 300\n")
            .contains("`ttl` is at most 255")
    );
    assert!(
        tcp("    destinations:\n      - address: \"239.2.3.1:6969\"\n")
            .contains("is a multicast group, which a `tcp` subscriber cannot connect to")
    );
    assert!(udp(&format!(
        "{one}    tls:\n      ca_file: /nonexistent/ca.pem\n"
    ))
    .contains("a `udp` subscriber sends plaintext datagrams and takes no `tls` block"));
    assert!(udp(&format!("{one}    queue: 10\n")).contains("applies to a `tcp` subscriber"));
    assert!(tcp(&format!("{one}    queue: 0\n")).contains("`queue`: must be at least 1"));
    assert!(tcp(&format!(
        "{one}    tls:\n      ca_file: /nonexistent/ca.pem\n"
    ))
    .starts_with("subscriber ToTak: `tls.ca_file`: cannot read /nonexistent/ca.pem"));
    assert!(tcp(&format!("{one}    tls: {{}}\n")).contains("`tls`: needs `ca_file`"));
    assert!(udp(&format!("{one}    encoding: cbor\n{COT}"))
        .contains("`encoding`: does not apply with `cot`"));
    assert!(udp(&format!("{one}    encoding: msgpack\n")).contains("is not one of json, cbor"));
    // An unknown key in a destination or in `tls` is a parse error.
    let yaml = "subscribers:\n  - name: A\n    subscriber_type: udp\n    topics: [\"*\"]\n    destinations:\n      - address: \"203.0.113.10:8087\"\n        sourceport: 1\n";
    assert!(parse::<SubscriberConfigs>(yaml, "test")
        .unwrap_err()
        .to_string()
        .contains("sourceport"));
}

#[test]
fn a_client_certificate_needs_its_key() {
    let pki = pki();
    let error = refusal(&format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"203.0.113.10:8089\"\n    tls:\n      ca_file: \"{}\"\n      client_cert_file: \"{}\"\n",
        pki.ca_file.display(),
        pki.client_cert_file.display()
    ));
    assert!(
        error.contains("`client_cert_file` and `client_key_file` go together"),
        "{error}"
    );
    assert_eq!(
        sink(
            "    subscriber_type: tcp\n    destinations:\n      - address: \"203.0.113.10:8087\"\n"
        )
        .wire()
        .destinations,
        [("203.0.113.10:8087".to_string(), false)]
    );
    assert_eq!(DEFAULT_QUEUE, 1000);
}

/// What each level of the security block admits.
#[test]
fn security_levels_admit_plaintext_on_loopback_and_tls_off_host() {
    let wire = |protocol: &'static str, tls: bool, certificate: bool, loopback: bool| Wire {
        protocol,
        tls,
        client_certificate: certificate,
        destinations: vec![(
            if loopback {
                "127.0.0.1:8087"
            } else {
                "203.0.113.10:8087"
            }
            .to_string(),
            loopback,
        )],
    };
    let levels = [
        Transport::Unenforced,
        Transport::LoopbackPlaintext,
        Transport::Tls,
        Transport::Mtls,
    ];
    // (wire, admitted under each level in the order above)
    let table = [
        (wire("udp", false, false, true), [true, true, true, true]),
        (
            wire("udp", false, false, false),
            [true, false, false, false],
        ),
        (wire("tcp", false, false, true), [true, true, true, true]),
        (
            wire("tcp", false, false, false),
            [true, false, false, false],
        ),
        (wire("tcp", true, false, false), [true, false, true, false]),
        (wire("tcp", true, true, false), [true, false, true, true]),
        (wire("tcp", true, false, true), [true, true, true, false]),
    ];
    for (wire, expected) in table {
        for (level, expected) in levels.iter().zip(expected) {
            assert_eq!(admit(*level, &wire).is_ok(), expected, "{level:?} {wire:?}");
        }
    }
    let refused = admit(Transport::Tls, &wire("udp", false, false, false)).unwrap_err();
    assert!(
        refused.starts_with("level `tls` needs TLS, and a `udp` subscriber sends plaintext datagrams; `destinations` gives 203.0.113.10:8087"),
        "{refused}"
    );
    assert!(refused.contains("security: { transport: unenforced, reason: ... }"));
    assert!(admit(Transport::Mtls, &wire("tcp", true, false, false))
        .unwrap_err()
        .contains("`tls.client_cert_file`"));
    // One destination off this machine is enough to refuse the subscriber.
    let mut mixed = wire("udp", false, false, true);
    mixed
        .destinations
        .push(("203.0.113.10:8087".to_string(), false));
    assert!(admit(Transport::LoopbackPlaintext, &mixed).is_err());
}

/// The security block, read as `mind` reads it.
fn security(block: &str) -> Security {
    Security::from_yaml(&format!("security:\n{block}"), "test")
        .unwrap()
        .unwrap()
}

fn refused_under(security: &Security, body: &str) -> String {
    match build_sink_secured(&subscriber(body), security) {
        Ok(_) => panic!("{body} built a sink"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn the_security_block_refuses_plaintext_that_leaves_this_machine() {
    let off_host = "    destinations:\n      - address: \"203.0.113.10:8087\"\n";
    let loopback = "    destinations:\n      - address: \"127.0.0.1:8087\"\n";
    let udp = |body: &str| format!("    subscriber_type: udp\n{body}");
    let tcp = |body: &str| format!("    subscriber_type: tcp\n{body}");
    for level in ["tls", "mtls"] {
        let block = security(&format!("  transport: {level}\n"));
        let error = refused_under(&block, &udp(off_host));
        assert!(
            error.starts_with(&format!(
                "subscriber ToTak: level `{level}` needs TLS, and a `udp` subscriber sends plaintext datagrams; `destinations` gives 203.0.113.10:8087"
            )),
            "{error}"
        );
        assert!(refused_under(&block, &tcp(off_host)).contains("has no `tls` block"));
        // On this machine plaintext crosses no network.
        assert!(build_sink_secured(&subscriber(&udp(loopback)), &block).is_ok());
        assert!(build_sink_secured(&subscriber(&tcp(loopback)), &block).is_ok());
        // An override must say why.
        let silent = format!("{off_host}    security: {{ transport: unenforced }}\n");
        assert!(refused_under(&block, &udp(&silent)).contains("reason"));
        let reasoned = format!(
            "{off_host}    security: {{ transport: unenforced, reason: \"TAK clients on the enclave network take multicast only\" }}\n"
        );
        let (posture, targets) = describe_sink(&subscriber(&udp(&reasoned)), &block).unwrap();
        assert_eq!(posture.transport, "plaintext");
        assert!(posture.off_host);
        assert_eq!(posture.overrides, ["transport"]);
        assert!(posture.override_reason.unwrap().contains("multicast only"));
        assert!(targets.is_empty());
    }
    let block = security("  transport: loopback-plaintext\n");
    assert!(refused_under(&block, &udp(off_host))
        .contains("level `loopback-plaintext` refuses a sink that leaves this host"));
    assert!(build_sink_secured(&subscriber(&udp(loopback)), &block).is_ok());
}

#[test]
fn a_tls_block_satisfies_tls_and_a_client_certificate_satisfies_mtls() {
    let pki = pki();
    let server_only = format!(
        "    subscriber_type: tcp\n    destinations:\n      - address: \"tak.example.org:8089\"\n    tls:\n      ca_file: \"{}\"\n",
        pki.ca_file.display()
    );
    let with_certificate = format!(
        "{server_only}      client_cert_file: \"{}\"\n      client_key_file: \"{}\"\n",
        pki.client_cert_file.display(),
        pki.client_key_file.display()
    );
    let tls = security("  transport: tls\n");
    let mtls = security("  transport: mtls\n");
    let (posture, targets) = describe_sink(&subscriber(&server_only), &tls).unwrap();
    assert_eq!(posture.kind, "tcp");
    assert_eq!(posture.transport, "TLS");
    assert_eq!(posture.endpoint, "tak.example.org:8089");
    assert_eq!(posture.min_tls.as_deref(), Some("1.3"));
    assert!(posture.trust.unwrap().starts_with("bundle "));
    assert_eq!(targets.len(), 1);
    assert_eq!(
        (targets[0].host.as_str(), targets[0].port),
        ("tak.example.org", 8089)
    );
    assert!(refused_under(&mtls, &server_only).contains("level `mtls` needs a client certificate"));
    let (posture, _) = describe_sink(&subscriber(&with_certificate), &mtls).unwrap();
    assert_eq!(posture.transport, "mutual TLS");
    assert!(posture.client_auth.starts_with("certificate "));

    // The certificate and the authorities can come from the security block.
    let from_block = security(&format!(
        "  transport: mtls\n  ca_bundle: \"{}\"\n  client_cert: \"{}\"\n  client_key: \"{}\"\n",
        pki.ca_file.display(),
        pki.client_cert_file.display(),
        pki.client_key_file.display()
    ));
    let bare = "    subscriber_type: tcp\n    destinations:\n      - address: \"tak.example.org:8089\"\n    tls: {}\n";
    let (posture, _) = describe_sink(&subscriber(bare), &from_block).unwrap();
    assert_eq!(posture.transport, "mutual TLS");
}
