use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::time::SystemTime;

pub fn get_env_or_default(env_name: &str, default: Option<&str>) -> Option<String> {
    env::var(env_name)
        .ok()
        .or_else(|| default.map(|s| s.to_string()))
}

/// The data model version a typed record states in `data_model_version`.
pub const TYPED_RECORD_VERSION: &str = "2.0.0";

/// Environment variable that selects the record version: `2` (the default)
/// or `1`.
pub const RECORD_VERSION_ENV: &str = "FIRST_BITE_RECORD_VERSION";

/// Which form of the record is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordVersion {
    /// Version 1: every decoded field value and both ports are text.
    Text,
    /// Version 2: values keep their type. Integers and scaled values are
    /// numbers, booleans are booleans, labels and text are strings, bytes are
    /// a hexadecimal string, ports are numbers. The record also says where it
    /// was captured and which dictionary decoded it.
    #[default]
    Typed,
}

impl RecordVersion {
    /// The version `FIRST_BITE_RECORD_VERSION` selects; version 2 when the
    /// variable is unset or empty. `Err` carries a value that is neither.
    pub fn from_env() -> Result<Self, String> {
        match env::var(RECORD_VERSION_ENV) {
            Ok(value) if !value.trim().is_empty() => Self::parse(&value).ok_or(value),
            _ => Ok(RecordVersion::default()),
        }
    }

    /// `1` or `2`, as written in configuration.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "1" => Some(RecordVersion::Text),
            "2" => Some(RecordVersion::Typed),
            _ => None,
        }
    }

    pub fn number(self) -> u8 {
        match self {
            RecordVersion::Text => 1,
            RecordVersion::Typed => 2,
        }
    }
}

/// A UDP or TCP port: a number in a version 2 record, decimal text in a
/// version 1 record.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum Port {
    Number(u16),
    Text(String),
}

impl Port {
    pub fn new(port: u16, version: RecordVersion) -> Self {
        match version {
            RecordVersion::Typed => Port::Number(port),
            RecordVersion::Text => Port::Text(port.to_string()),
        }
    }

    /// The port number, whichever way the record carries it.
    pub fn number(&self) -> Option<u16> {
        match self {
            Port::Number(port) => Some(*port),
            Port::Text(text) => text.parse().ok(),
        }
    }
}

/// One captured UDP or TCP packet and, when the dictionary knows its port
/// and message type, the decoded message. This is the record every publisher
/// and sink carries; its serialised field names and value types are the
/// contract `data_model_version` names.
#[derive(Serialize, Deserialize, Debug)]
pub struct PacketInfoWithParsedMessage {
    pub timestamp: SystemTime,
    pub src_port: Port,
    pub dst_port: Port,
    pub src_ip: Vec<u8>,
    pub dst_ip: Vec<u8>,
    /// The transport payload, base64.
    pub data_encoded_inner: String,
    /// The dictionary interface the packet's port belongs to.
    pub subsystem: Option<String>,
    pub message_type: Option<String>,
    /// Field name to value.
    pub message: Option<Value>,
    /// The frame as captured, base64.
    pub data_encoded_full: String,
    pub data_source_geo_location: Option<String>,
    pub data_source_network_name: Option<String>,
    pub data_source_system_type: Option<String>,
    pub data_source_owner: Option<String>,
    pub data_packet_classification: Option<String>,
    pub data_packet_accm: Option<String>,
    pub data_model_version: Option<String>,
    pub application_version: Option<String>,
    /// The network interface the frame was captured on. Version 2, live
    /// capture only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_interface: Option<String>,
    /// The tunnel the packet was unwrapped from: `gre`, `erspan` or `vxlan`.
    /// Version 2, tunnelled packets only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<String>,
    /// Name of the dictionary that decoded the message. Version 2, decoded
    /// records only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dictionary: Option<String>,
    /// Version of that dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dictionary_version: Option<String>,
}

impl PacketMetadata {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[derive(Serialize)]
pub struct PacketMetadata {
    pub data_source_geo_location: Option<String>,
    pub data_source_network_name: Option<String>,
    pub data_source_system_type: Option<String>,
    pub data_source_owner: Option<String>,
    pub data_packet_classification: Option<String>,
    pub data_packet_accm: Option<String>,
    pub data_model_version: Option<String>,
    pub application_version: Option<String>,
}

impl std::fmt::Display for PacketMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "\n**PacketMetadata**
Data Source Geo Location: {:?}
Data Source Network Name: {:?}
Data Source System Type: {:?}
Data Source Owner: {:?}
Data Packet Classification: {:?}
Data Packet ACCM: {:?}
Data Model version: {:?}
Application version: {:?}
**end**\n",
            self.data_source_geo_location,
            self.data_source_network_name,
            self.data_source_system_type,
            self.data_source_owner,
            self.data_packet_classification,
            self.data_packet_accm,
            self.data_model_version,
            self.application_version
        )
    }
}

/// The deployment's metadata for records of `version`, read from the
/// environment. A version 2 record always states its data model version; a
/// version 1 record states `FIRST_BITE_DATA_MODEL_VERSION` when that is set.
pub fn get_packet_metadata(version: RecordVersion) -> PacketMetadata {
    let application_version = option_env!("CARGO_PKG_VERSION").map(|s| s.to_string());

    PacketMetadata {
        data_source_geo_location: get_env_or_default("DATA_SOURCE_GEO_LOCATION", None),
        data_source_network_name: get_env_or_default("DATA_SOURCE_NETWORK_NAME", None),
        data_source_system_type: get_env_or_default("DATA_SOURCE_SYSTEM_TYPE", None),
        data_source_owner: get_env_or_default("DATA_SOURCE_OWNER", None),
        data_packet_classification: get_env_or_default("DATA_PACKET_CLASSIFICATION", None),
        data_packet_accm: get_env_or_default("DATA_PACKET_ACCM", None),
        data_model_version: match version {
            RecordVersion::Typed => Some(TYPED_RECORD_VERSION.to_string()),
            RecordVersion::Text => get_env_or_default("FIRST_BITE_DATA_MODEL_VERSION", None),
        },
        application_version,
    }
}

#[derive(Serialize)]
pub struct LogMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_geo_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_network_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_system_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_version: Option<String>,
}

impl LogMetadata {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl std::fmt::Display for LogMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "\n** System Metadata **
Data Source Geo Location: {:?}
Data Source Network Name: {:?}
Data Source System Type: {:?}
Data Source Owner: {:?}
Data Model version: {:?}
Application version: {:?}
**end**\n",
            self.data_source_geo_location,
            self.data_source_network_name,
            self.data_source_system_type,
            self.data_source_owner,
            self.data_model_version,
            self.application_version
        )
    }
}

pub fn get_log_metadata(application_version: Option<String>) -> LogMetadata {
    let data_model_version = match RecordVersion::from_env() {
        Ok(RecordVersion::Text) => {
            get_env_or_default("FIRST_BITE_DATA_MODEL_VERSION", Some("1.0.1"))
        }
        _ => Some(TYPED_RECORD_VERSION.to_string()),
    };
    LogMetadata {
        data_source_geo_location: get_env_or_default("DATA_SOURCE_GEO_LOCATION", None),
        data_source_network_name: get_env_or_default("DATA_SOURCE_NETWORK_NAME", None),
        data_source_system_type: get_env_or_default("DATA_SOURCE_SYSTEM_TYPE", None),
        data_source_owner: get_env_or_default("DATA_SOURCE_OWNER", None),
        data_model_version,
        application_version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_versions_are_written_as_one_and_two() {
        assert_eq!(RecordVersion::parse("1"), Some(RecordVersion::Text));
        assert_eq!(RecordVersion::parse(" 2 "), Some(RecordVersion::Typed));
        assert_eq!(RecordVersion::parse("3"), None);
        assert_eq!(RecordVersion::default().number(), 2);
    }

    #[test]
    fn a_port_is_a_number_or_text_and_reads_back_from_either() {
        let typed = Port::new(40100, RecordVersion::Typed);
        let text = Port::new(40100, RecordVersion::Text);
        assert_eq!(serde_json::to_string(&typed).unwrap(), "40100");
        assert_eq!(serde_json::to_string(&text).unwrap(), "\"40100\"");
        assert_eq!(serde_json::from_str::<Port>("40100").unwrap(), typed);
        assert_eq!(serde_json::from_str::<Port>("\"40100\"").unwrap(), text);
        assert_eq!(typed.number(), Some(40100));
        assert_eq!(text.number(), Some(40100));
    }
}
