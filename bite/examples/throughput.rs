//! Measure how fast the decoder runs on this machine.
//!
//!     cargo run --release -p bite --example throughput -- dictionaries/weather-station
//!
//! Decodes every vector of the dictionary in a loop on one thread for about two
//! seconds and prints messages and fields per second. Rendering the fields to
//! the published form is included, because that is what the pipeline does.

use bite::schema::VectorSpec;
use bite::Dictionary;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: throughput <dictionary directory>")?;
    let dictionary = Dictionary::load(&path)?;

    let mut cases = Vec::new();
    for entry in std::fs::read_dir(dictionary.root.join("vectors"))? {
        let vector: VectorSpec = serde_json::from_str(&std::fs::read_to_string(entry?.path())?)?;
        let interface = dictionary
            .interface(&vector.interface)
            .ok_or("vector names an unknown interface")?;
        let payload = hex::decode(vector.payload_hex.replace(char::is_whitespace, ""))?;
        cases.push((interface, payload, vector.frame_id));
    }
    if cases.is_empty() {
        return Err("the dictionary has no vectors".into());
    }

    let budget = Duration::from_secs(2);
    let started = Instant::now();
    let (mut messages, mut fields, mut bytes) = (0u64, 0u64, 0u64);
    while started.elapsed() < budget {
        for _ in 0..1_000 {
            for (interface, payload, frame_id) in &cases {
                if let Ok(decoded) = interface.decode_frame(payload, *frame_id) {
                    fields += decoded.fields.len() as u64;
                    bytes += payload.len() as u64;
                    std::hint::black_box(decoded.rendered());
                }
                messages += 1;
            }
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    println!("dictionary        {path}");
    println!("messages decoded  {messages}");
    println!("messages/second   {:.0}", messages as f64 / seconds);
    println!("fields/second     {:.0}", fields as f64 / seconds);
    println!("payload MB/second {:.1}", bytes as f64 / seconds / 1e6);
    println!("ns per message    {:.0}", seconds * 1e9 / messages as f64);
    Ok(())
}
