//! first-bite process control: capture frames, decode them against a
//! dictionary, publish the records.

pub mod cli;
pub mod decap;
pub mod dictionary;
pub mod discover;
pub mod error;
pub mod goat;
pub mod pipeline;
pub mod run;
pub mod security;
pub mod slices;
pub mod source;
pub mod splash;
pub mod topics;

pub use crate::error::Error;
pub use crate::pipeline::{Decapsulation, Pipeline};
pub use crate::run::{run, OutputFile, RecordSink, Stats};
pub use crate::source::{
    CaptureOptions, CaptureStats, FileCapture, Frame, FrameSource, LinkType, LiveCapture,
    MultiCapture, UdpFrames,
};
