//! Turn decoded records into Cursor-on-Target (CoT) events.
//!
//! Nothing here knows a message format. A [`Mapping`], supplied as
//! configuration, names the fields of a decoded message that give the event
//! its identifier, type and position; [`record_to_cot`] applies it to one
//! published record.

pub mod event;
pub mod mapping;

pub use crate::event::{
    ecef_to_geodetic, event_times, event_xml, record_to_cot, record_to_cot_at, CotError, CotEvent,
};
pub use crate::mapping::{EventType, Mapping, MappingError, Position, DEFAULT_STALE_MINUTES};
