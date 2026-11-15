//! Decoding never panics, whatever the payload: the input is whatever was on
//! the wire.

use bite::Dictionary;
use std::path::PathBuf;

/// xorshift64*: small, deterministic, good enough to vary payloads.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn shipped() -> Vec<Dictionary> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dictionaries");
    let mut dictionaries = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            dictionaries.push(Dictionary::load(path).unwrap());
        }
    }
    assert!(dictionaries.len() >= 3);
    dictionaries
}

#[test]
fn random_payloads_never_panic() {
    let dictionaries = shipped();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut decoded_fields = 0usize;
    for round in 0..20_000 {
        let length = (rng.next() % 96) as usize;
        let mut payload: Vec<u8> = (0..length).map(|_| rng.next() as u8).collect();
        for dictionary in &dictionaries {
            for interface in dictionary.interfaces() {
                // Half the rounds force a known message type so that field
                // decoding, not only identification, sees the random bytes.
                let frame_id = if round % 2 == 0 {
                    let codes: Vec<u64> = interface.discriminator.names.keys().copied().collect();
                    let code = codes[(rng.next() as usize) % codes.len()];
                    let (start, stop) =
                        (interface.discriminator.start, interface.discriminator.stop);
                    if stop > start && stop % 8 == 0 && start % 8 == 0 && payload.len() * 8 >= stop
                    {
                        let bytes = code.to_be_bytes();
                        let width = (stop - start) / 8;
                        payload[start / 8..stop / 8].copy_from_slice(&bytes[8 - width..]);
                    }
                    code
                } else {
                    rng.next()
                };
                if let Ok(decoded) = interface.decode_frame(&payload, Some(frame_id)) {
                    decoded_fields += decoded.fields.len();
                    // Rendering and serialising are part of the hot path too.
                    let _ = decoded.rendered();
                    serde_json::to_string(&decoded).unwrap();
                }
            }
        }
    }
    assert!(
        decoded_fields > 100_000,
        "only {decoded_fields} fields decoded"
    );
}

#[test]
fn every_prefix_of_every_vector_decodes() {
    for dictionary in shipped() {
        let vectors = dictionary.root.join("vectors");
        for entry in std::fs::read_dir(vectors).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            let vector: bite::schema::VectorSpec = serde_json::from_str(&text).unwrap();
            let payload = hex::decode(vector.payload_hex.replace(char::is_whitespace, "")).unwrap();
            let interface = dictionary.interface(&vector.interface).unwrap();
            for length in 0..=payload.len() {
                let _ = interface.decode_frame(&payload[..length], vector.frame_id);
            }
        }
    }
}
