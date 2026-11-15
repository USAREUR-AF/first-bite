//! The JavaScript interface: [`crate::api`] with JavaScript values in and out
//! and failures thrown as `Error`.

use crate::api::Session;
use js_sys::JSON;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

fn to_js(value: serde_json::Value) -> JsValue {
    JSON::parse(&value.to_string()).expect("serde_json writes JSON that JSON.parse reads")
}

fn exact_id(value: Option<f64>) -> Result<Option<u64>, JsError> {
    match value {
        None => Ok(None),
        // Integers above 2^53 are not exact as JavaScript numbers.
        Some(id) if id >= 0.0 && id.fract() == 0.0 && id <= 9_007_199_254_740_992.0 => {
            Ok(Some(id as u64))
        }
        Some(id) => Err(JsError::new(&format!(
            "frame identifier {id} is not a non-negative integer"
        ))),
    }
}

/// Build a dictionary from an object of relative file path to file text.
#[wasm_bindgen(js_name = loadDictionary)]
pub fn load_dictionary(files: JsValue) -> Result<Dictionary, JsError> {
    let malformed = || JsError::new("files must be an object of file path to file text");
    let text: String = JSON::stringify(&files)
        .map_err(|_| malformed())?
        .as_string()
        .ok_or_else(malformed)?;
    let files: BTreeMap<String, String> = serde_json::from_str(&text).map_err(|_| malformed())?;
    Session::load(files)
        .map(|session| Dictionary { session })
        .map_err(|message| JsError::new(&message))
}

/// A loaded dictionary.
#[wasm_bindgen]
pub struct Dictionary {
    session: Session,
}

#[wasm_bindgen]
impl Dictionary {
    /// `{name, version, description}` from `dictionary.json`.
    pub fn info(&self) -> JsValue {
        to_js(self.session.info())
    }

    /// The interfaces, each with its port, how it identifies messages, and
    /// its messages with their fields.
    pub fn interfaces(&self) -> JsValue {
        to_js(self.session.interfaces())
    }

    /// What `bite check` reports about the dictionary files.
    pub fn issues(&self) -> JsValue {
        to_js(self.session.issues())
    }

    /// Decode a payload given as hex against the named interface.
    pub fn decode(
        &self,
        interface: &str,
        payload_hex: &str,
        frame_id: Option<f64>,
    ) -> Result<JsValue, JsError> {
        self.session
            .decode(interface, payload_hex, exact_id(frame_id)?)
            .map(to_js)
            .map_err(|message| JsError::new(&message))
    }

    /// Decode a payload given as hex against the interface that claims a port.
    #[wasm_bindgen(js_name = decodeByPort)]
    pub fn decode_by_port(
        &self,
        port: u16,
        payload_hex: &str,
        frame_id: Option<f64>,
    ) -> Result<JsValue, JsError> {
        self.session
            .decode_by_port(port, payload_hex, exact_id(frame_id)?)
            .map(to_js)
            .map_err(|message| JsError::new(&message))
    }

    /// JSON Schema for the decoded form of every message, keyed
    /// `interface/message`.
    pub fn schema(&self) -> JsValue {
        to_js(self.session.schema())
    }

    /// The test vectors the dictionary carries.
    pub fn vectors(&self) -> JsValue {
        to_js(self.session.vectors())
    }

    /// Run the test vectors: `[{name, passed, failures}]`.
    #[wasm_bindgen(js_name = runVectors)]
    pub fn run_vectors(&self) -> JsValue {
        to_js(self.session.run_vectors())
    }
}
