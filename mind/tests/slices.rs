//! Rotating pcap slices and their publisher. Every frame is built by hand.

use etherparse::PacketBuilder;
use mind::slices::{
    check_publish, expand_key, finished_slices, PublishOptions, SliceOptions, SlicePublisher,
    SliceWriter, PCAP_CONTENT_TYPE,
};
use mind::{FileCapture, Frame, FrameSource, LinkType};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mind-slices-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn udp_frame(payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
        .udp(47000, 47000);
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut frame, payload).unwrap();
    frame
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seconds)
}

fn frame(data: &[u8], seconds: u64) -> Frame<'_> {
    Frame {
        timestamp: at(seconds),
        link: LinkType::Ethernet,
        data,
        interface: None,
    }
}

fn options(dir: &Path) -> SliceOptions {
    SliceOptions {
        dir: dir.to_path_buf(),
        prefix: "tap".to_string(),
        seconds: Some(60),
        max_bytes: None,
        keep: 100,
    }
}

/// (link type, [(seconds, payload)]) of a pcap file, read by hand.
fn read_pcap(path: &Path) -> (u32, Vec<(u32, Vec<u8>)>) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], &0xa1b2_c3d4u32.to_le_bytes());
    let link = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let mut records = Vec::new();
    let mut at = 24;
    while at < bytes.len() {
        let seconds = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let length = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        records.push((seconds, bytes[at + 16..at + 16 + length].to_vec()));
        at += 16 + length;
    }
    (link, records)
}

#[test]
fn slices_are_cut_by_time_and_hold_every_frame() {
    let dir = scratch("time");
    let a = udp_frame(b"one");
    let b = udp_frame(b"two");
    let c = udp_frame(b"three");
    {
        let mut writer = SliceWriter::new(options(&dir), None).unwrap();
        writer.write(&frame(&a, 0)).unwrap();
        writer.write(&frame(&b, 59)).unwrap();
        writer.write(&frame(&c, 60)).unwrap();
    } // dropping the writer closes the last slice
    let slices = finished_slices(&dir, "tap").unwrap();
    assert_eq!(slices.len(), 2, "{slices:?}");
    assert!(slices[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("tap-20231114T221320Z-"));
    let (link, first) = read_pcap(&slices[0]);
    assert_eq!(link, 1);
    assert_eq!(first, vec![(1_700_000_000, a), (1_700_000_059, b)]);
    let (_, second) = read_pcap(&slices[1]);
    assert_eq!(second, vec![(1_700_000_060, c)]);
    assert!(fs::read_dir(&dir).unwrap().all(|entry| !entry
        .unwrap()
        .path()
        .to_string_lossy()
        .ends_with(".part")));
}

#[test]
fn slices_are_cut_by_size_and_a_link_type_change() {
    let dir = scratch("size");
    let data = udp_frame(&[7u8; 100]); // 142 bytes on the wire, 158 in the file
    {
        let mut writer = SliceWriter::new(
            SliceOptions {
                seconds: None,
                max_bytes: Some(24 + 2 * 158),
                ..options(&dir)
            },
            None,
        )
        .unwrap();
        for second in 0..5 {
            writer.write(&frame(&data, second)).unwrap();
        }
        let raw = Frame {
            link: LinkType::RawIp,
            ..frame(&data[14..], 5)
        };
        writer.write(&raw).unwrap();
    }
    let slices = finished_slices(&dir, "tap").unwrap();
    let counts: Vec<usize> = slices.iter().map(|path| read_pcap(path).1.len()).collect();
    assert_eq!(counts, vec![2, 2, 1, 1]);
    assert_eq!(read_pcap(&slices[3]).0, 101);
}

#[test]
fn only_the_newest_slices_are_kept() {
    let dir = scratch("keep");
    let data = udp_frame(b"x");
    {
        let mut writer = SliceWriter::new(
            SliceOptions {
                keep: 2,
                ..options(&dir)
            },
            None,
        )
        .unwrap();
        for minute in 0..5 {
            writer.write(&frame(&data, minute * 60)).unwrap();
        }
    }
    let slices = finished_slices(&dir, "tap").unwrap();
    assert_eq!(slices.len(), 2);
    let seconds: Vec<u32> = slices.iter().map(|path| read_pcap(path).1[0].0).collect();
    assert_eq!(seconds, vec![1_700_000_180, 1_700_000_240]);
}

#[test]
fn a_quiet_link_still_closes_the_slice_and_hands_it_on() {
    let dir = scratch("tick");
    let (sender, receiver) = mpsc::channel();
    let mut writer = SliceWriter::new(options(&dir), Some(sender)).unwrap();
    let data = udp_frame(b"x");
    writer.write(&frame(&data, 0)).unwrap();
    writer.tick(at(30)).unwrap();
    assert!(receiver.try_recv().is_err());
    writer.tick(at(61)).unwrap();
    let closed = receiver.try_recv().unwrap();
    assert!(closed.exists());
    assert_eq!(read_pcap(&closed).1.len(), 1);
    writer.tick(at(500)).unwrap(); // nothing open: nothing happens
    assert!(receiver.try_recv().is_err());
}

#[test]
fn slices_are_files_libpcap_reads() {
    let dir = scratch("libpcap");
    let a = udp_frame(b"alpha");
    let b = udp_frame(b"beta");
    {
        let mut writer = SliceWriter::new(options(&dir), None).unwrap();
        writer.write(&frame(&a, 0)).unwrap();
        writer.write(&frame(&b, 1)).unwrap();
    }
    let path = &finished_slices(&dir, "tap").unwrap()[0];
    let mut source = FileCapture::open(path, Some("udp port 47000")).unwrap();
    let mut read = Vec::new();
    while let Some(frame) = source.next_frame().unwrap() {
        read.push(frame.data.to_vec());
    }
    assert_eq!(read, vec![a, b]);
}

/// One request as the test server saw it: method, path, content type, body.
type Request = (String, String, String, Vec<u8>);

/// A one-connection-at-a-time HTTP server answering each request with the
/// next status of `statuses`; it reports (method, path, content type, body).
fn server(statuses: Vec<u16>) -> (String, mpsc::Receiver<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for status in statuses {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap().to_string(),
                parts.next().unwrap().to_string(),
            );
            let (mut length, mut kind) = (0usize, String::new());
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => length = value.trim().parse().unwrap(),
                    "content-type" => kind = value.trim().to_string(),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            )
            .unwrap();
            sender.send((method, path, kind, body)).unwrap();
        }
    });
    (url, receiver)
}

fn publish_options(url: String, retries: u32) -> PublishOptions {
    PublishOptions {
        url,
        key: "sensor/pcap/node-1".to_string(),
        retries,
        first_wait: Duration::from_millis(10),
    }
}

#[test]
fn a_finished_slice_is_put_on_its_key_and_retried_after_an_error() {
    let dir = scratch("publish");
    let (url, requests) = server(vec![500, 200]);
    let publisher = SlicePublisher::spawn(publish_options(url, 3)).unwrap();
    {
        let mut writer = SliceWriter::new(options(&dir), Some(publisher.sender())).unwrap();
        writer.write(&frame(&udp_frame(b"x"), 0)).unwrap();
    }
    let counts = publisher.finish(Duration::from_secs(10)).unwrap();
    assert_eq!((counts.published, counts.failed), (1, 0));
    let slice = fs::read(&finished_slices(&dir, "tap").unwrap()[0]).unwrap();
    for _ in 0..2 {
        let (method, path, kind, body) = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(method, "PUT");
        assert_eq!(path, "/sensor/pcap/node-1");
        assert_eq!(kind, PCAP_CONTENT_TYPE);
        assert_eq!(body, slice);
    }
}

#[test]
fn a_slice_that_keeps_failing_stays_on_disk() {
    let dir = scratch("fail");
    let (url, requests) = server(vec![503, 503]);
    let publisher = SlicePublisher::spawn(publish_options(url, 1)).unwrap();
    {
        let mut writer = SliceWriter::new(options(&dir), Some(publisher.sender())).unwrap();
        writer.write(&frame(&udp_frame(b"x"), 0)).unwrap();
    }
    let counts = publisher.finish(Duration::from_secs(10)).unwrap();
    assert_eq!((counts.published, counts.failed), (0, 1));
    assert_eq!(requests.iter().take(2).count(), 2);
    assert_eq!(finished_slices(&dir, "tap").unwrap().len(), 1);
}

#[test]
fn publish_settings_are_checked() {
    let ok = publish_options("http://127.0.0.1:8000".to_string(), 0);
    assert!(check_publish(&ok).is_ok());
    for (url, key) in [
        ("https://127.0.0.1:8000", "a/b"),
        ("not a url", "a/b"),
        ("http://127.0.0.1:8000", ""),
        ("http://127.0.0.1:8000", "/a"),
        ("http://127.0.0.1:8000", "a/"),
    ] {
        let options = PublishOptions {
            url: url.to_string(),
            key: key.to_string(),
            ..ok.clone()
        };
        assert!(check_publish(&options).is_err(), "{url} {key}");
    }
    assert_eq!(expand_key("a/b"), "a/b");
    let expanded = expand_key("sensor/pcap/{host}");
    assert!(expanded.starts_with("sensor/pcap/") && !expanded.contains('{'));
}

#[test]
fn mind_tap_only_writes_slices_from_a_capture_file() {
    // The input capture is itself written by the slice writer.
    let input = scratch("cli-input");
    {
        let mut writer = SliceWriter::new(
            SliceOptions {
                seconds: None,
                ..options(&input)
            },
            None,
        )
        .unwrap();
        for second in [0, 30, 61, 62, 130] {
            writer.write(&frame(&udp_frame(b"x"), second)).unwrap();
        }
    }
    let input_file = finished_slices(&input, "tap").unwrap().remove(0);
    let output = scratch("cli-output");
    let status = Command::new(env!("CARGO_BIN_EXE_mind"))
        .arg("--pcap-file")
        .arg(&input_file)
        .args([
            "--filter",
            "udp port 47000",
            "--tap-only",
            "--pcap-prefix",
            "radar",
        ])
        .arg("--pcap-dir")
        .arg(&output)
        .env_remove("BITE_DICT_PATH")
        .env_remove("PUBLISHER_CONFIG_PATH")
        .env_remove("SUBSCRIBER_CONFIG_PATH")
        .status()
        .unwrap();
    assert!(status.success());
    let counts: Vec<usize> = finished_slices(&output, "radar")
        .unwrap()
        .iter()
        .map(|path| read_pcap(path).1.len())
        .collect();
    assert_eq!(counts, vec![2, 2, 1]);
}
