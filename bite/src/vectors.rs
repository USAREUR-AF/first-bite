//! Test vectors carried by a dictionary: payloads and what they decode to.

use crate::decode::Value;
use crate::dictionary::{Dictionary, LoadError};
use crate::schema::VectorSpec;

/// The outcome of one vector file. Empty `failures` means it passed.
#[derive(Debug, Clone)]
pub struct VectorResult {
    pub name: String,
    pub failures: Vec<String>,
}

impl VectorResult {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// A vector file: its name (the file stem) and the vector, or why the file
/// could not be read or parsed.
pub type NamedVector = (String, Result<VectorSpec, String>);

/// Every vector the dictionary carries, by file name.
pub fn load_vectors(dictionary: &Dictionary) -> Result<Vec<NamedVector>, LoadError> {
    Ok(dictionary
        .vector_files()?
        .iter()
        .map(|file| {
            let vector = file.text.clone().and_then(|text| {
                serde_json::from_str::<VectorSpec>(&text).map_err(|e| e.to_string())
            });
            (file.name.clone(), vector)
        })
        .collect())
}

/// Run every vector the dictionary carries: the `*.json` files of its
/// `vectors/` directory.
pub fn run_vectors(dictionary: &Dictionary) -> Result<Vec<VectorResult>, LoadError> {
    Ok(load_vectors(dictionary)?
        .into_iter()
        .map(|(name, vector)| VectorResult {
            name,
            failures: match vector {
                Ok(vector) => check_vector(dictionary, &vector),
                Err(message) => vec![message],
            },
        })
        .collect())
}

/// Decode one vector and list every way the result differs from what it expects.
pub fn check_vector(dictionary: &Dictionary, vector: &VectorSpec) -> Vec<String> {
    let Some(interface) = dictionary.interface(&vector.interface) else {
        return vec![format!("no interface named {:?}", vector.interface)];
    };
    let compact: String = vector
        .payload_hex
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let payload = match hex::decode(&compact) {
        Ok(payload) => payload,
        Err(e) => return vec![format!("payload_hex: {e}")],
    };
    let decoded = match interface.decode_frame(&payload, vector.frame_id) {
        Ok(decoded) => decoded,
        Err(e) => return vec![e.to_string()],
    };

    let mut failures = Vec::new();
    if let Some(expected) = &vector.expect.message_type {
        let actual = decoded.message_type();
        if &actual != expected {
            failures.push(format!(
                "message_type: expected {expected:?}, got {actual:?}"
            ));
        }
    }
    let rendered = decoded.rendered();
    for (field, expected) in &vector.expect.fields {
        match rendered.get(field.as_str()) {
            None => failures.push(format!("{field}: no such field in the decoded message")),
            Some(actual) if actual != expected => {
                let is_float = matches!(
                    decoded.field(field).and_then(|d| d.value.as_ref()),
                    Some(Value::Float(_))
                );
                if !(is_float && same_number(expected.as_deref(), actual.as_deref())) {
                    failures.push(format!("{field}: expected {expected:?}, got {actual:?}"))
                }
            }
            Some(_) => {}
        }
    }
    failures
}

/// Whether two rendered values are the same number to nine significant
/// digits. A vector may write `21.0` where the decoder writes `21`, and
/// `0.666666667` for a field whose scale is not a decimal, where the decoder
/// computes `0.6666666666666666`.
fn same_number(expected: Option<&str>, actual: Option<&str>) -> bool {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        return false;
    };
    match (expected.trim().parse::<f64>(), actual.parse::<f64>()) {
        (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => {
            (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-300)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::same_number;

    #[test]
    fn floats_compare_by_value() {
        assert!(same_number(Some("0.666666667"), Some("0.6666666666666666")));
        assert!(same_number(Some("12.6"), Some("12.599999999999994")));
        assert!(same_number(Some("21.0"), Some("21")));
        assert!(same_number(Some("0"), Some("0")));
        assert!(!same_number(Some("12.6"), Some("12.65")));
        assert!(!same_number(Some("12.6"), None));
        assert!(!same_number(Some("Idle"), Some("0")));
        assert!(!same_number(Some("0"), Some("0.000001")));
    }
}
