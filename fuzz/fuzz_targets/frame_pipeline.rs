//! Run an arbitrary captured frame through the pipeline: tunnel unwrapping
//! (GRE, ERSPAN, VXLAN), transport location, dictionary lookup by port,
//! decoding and the record that is published.
//!
//! Input: one byte of options, then the frame as captured.
//!
//!   bit 0     the frame starts at an IP header (raw-IP link), not Ethernet
//!   bit 1     GRE and ERSPAN are left as they are
//!   bit 2     VXLAN is unwrapped on UDP port 4789
//!   bits 4-7  which shipped dictionary is loaded; past the last one, none
#![no_main]

use first_bite_fuzz::shipped;
use libfuzzer_sys::fuzz_target;
use mind::{Decapsulation, Frame, LinkType, Pipeline};
use std::sync::{Arc, OnceLock};
use std::time::UNIX_EPOCH;

/// One pipeline per combination of the option bits that configure it.
static PIPELINES: OnceLock<Vec<Pipeline>> = OnceLock::new();

fn pipelines() -> Vec<Pipeline> {
    let mut dictionaries: Vec<Option<Arc<bite::Dictionary>>> = shipped()
        .into_iter()
        .map(|(_, dictionary)| Some(Arc::new(dictionary)))
        .collect();
    dictionaries.push(None);
    let mut pipelines = Vec::new();
    for dictionary in &dictionaries {
        for options in 0..4u8 {
            pipelines.push(
                Pipeline::new(dictionary.clone()).with_decapsulation(Decapsulation {
                    keep_gre: options & 1 != 0,
                    vxlan_ports: if options & 2 != 0 {
                        vec![4789]
                    } else {
                        Vec::new()
                    },
                }),
            );
        }
    }
    pipelines
}

fuzz_target!(|data: &[u8]| {
    let Some((&options, frame)) = data.split_first() else {
        return;
    };
    let pipelines = PIPELINES.get_or_init(pipelines);
    let dictionaries = pipelines.len() / 4;
    let dictionary = usize::from(options >> 4).min(dictionaries - 1);
    let pipeline = &pipelines[dictionary * 4 + usize::from((options >> 1) & 3)];
    let link = if options & 1 != 0 {
        LinkType::RawIp
    } else {
        LinkType::Ethernet
    };
    let frame = Frame {
        timestamp: UNIX_EPOCH,
        link,
        data: frame,
        interface: None,
    };
    if let Ok(record) = pipeline.process(&frame) {
        let _ = serde_json::to_string(&record);
    }
    let _ = pipeline.unwrapped();
});
