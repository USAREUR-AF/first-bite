//! The `bite` decoder for browsers: load a dictionary from files held in
//! memory and decode payloads against it, with no server.
//!
//! [`api`] is the whole of the behaviour, in plain Rust with JSON values in
//! and out. The WebAssembly build wraps it in the JavaScript interface
//! described in this crate's README.

pub mod api;

#[cfg(target_arch = "wasm32")]
mod bindings;
