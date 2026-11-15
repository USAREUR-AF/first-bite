use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::time::SystemTime;

pub fn get_env_or_default(env_name: &str, default: Option<&str>) -> Option<String> {
    env::var(env_name)
        .ok()
        .or_else(|| default.map(|s| s.to_string()))
}

// TODO -- Decide if we want to keep using the PacketInfo Struct
// or if we want to switch entirely to PacketInfoWithParsedMessage
#[derive(Serialize, Deserialize, Debug)]
pub struct PacketInfo {
    pub timestamp: SystemTime,
    pub src_port: String,
    pub dst_port: String,
    pub src_ip: Vec<u8>,
    pub dst_ip: Vec<u8>,
    pub data_encoded_inner: String, // for the raw message inside the tcp or udp packet
    pub subsystem: Option<String>,
    pub message_type: Option<String>,
    pub message: Option<String>,   // deserialized message
    pub data_encoded_full: String, // for the entire packet
    // metadata fields
    pub data_source_geo_location: Option<String>,
    pub data_source_network_name: Option<String>,
    pub data_source_system_type: Option<String>,
    pub data_source_owner: Option<String>,
    pub data_packet_classification: Option<String>,
    pub data_packet_accm: Option<String>,
    pub data_model_version: Option<String>,
    pub application_version: Option<String>,
}

impl std::fmt::Display for PacketInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "\n**PacketInfo**
Timestamp: {:?}
Src Port: {}
Dst Port: {}
Src IP: {:?}
Dst IP: {:?}
Data Encoded (Inner): {}
Subsystem: {:?}
Message Type: {:?}
Message: {:?}
Data Encoded (Full): {}
Data Source Geo Location: {:?}
Data Source Network Name: {:?}
Data Source System Type: {:?}
Data Source Owner: {:?}
Data Packet Classification: {:?}
Data Packet ACCM: {:?}\n     
Data Model version: {:?}
Application version: {:?}
**end**\n",
            self.timestamp,
            self.src_port,
            self.dst_port,
            self.src_ip,
            self.dst_ip,
            self.data_encoded_inner,
            self.subsystem,
            self.message_type,
            self.message.as_deref().unwrap_or("None"),
            self.data_encoded_full,
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

// We are using this struct to store the parsed message
// this way we can serialize it to json, log it, and send it to elasticsearch
// or any other data store
#[derive(Serialize, Deserialize, Debug)]
pub struct PacketInfoWithParsedMessage {
    pub timestamp: SystemTime,
    pub src_port: String,
    pub dst_port: String,
    pub src_ip: Vec<u8>,
    pub dst_ip: Vec<u8>,
    pub data_encoded_inner: String, // for the raw message inside the tcp or udp packet
    pub subsystem: Option<String>,
    pub message_type: Option<String>,
    pub message: Option<Value>,
    pub data_encoded_full: String, // for the entire packet
    pub data_source_geo_location: Option<String>,
    pub data_source_network_name: Option<String>,
    pub data_source_system_type: Option<String>,
    pub data_source_owner: Option<String>,
    pub data_packet_classification: Option<String>,
    pub data_packet_accm: Option<String>,
    pub data_model_version: Option<String>,
    pub application_version: Option<String>,
}

impl From<&PacketInfo> for PacketInfoWithParsedMessage {
    fn from(packet_info: &PacketInfo) -> Self {
        let parsed_message = packet_info.message.as_deref().map(|msg_str| {
            serde_json::from_str(msg_str).unwrap_or_else(|e| {
                eprintln!("Failed to parse message field: {}", e);
                serde_json::json!({"error": "Failed to parse message"})
            })
        });
        PacketInfoWithParsedMessage {
            timestamp: packet_info.timestamp,
            src_port: packet_info.src_port.clone(),
            dst_port: packet_info.dst_port.clone(),
            src_ip: packet_info.src_ip.clone(),
            dst_ip: packet_info.dst_ip.clone(),
            data_encoded_inner: packet_info.data_encoded_inner.clone(),
            subsystem: packet_info.subsystem.clone(),
            message_type: packet_info.message_type.clone(),
            message: parsed_message,
            data_encoded_full: packet_info.data_encoded_full.clone(),
            data_source_geo_location: packet_info.data_source_geo_location.clone(),
            data_source_network_name: packet_info.data_source_network_name.clone(),
            data_source_system_type: packet_info.data_source_system_type.clone(),
            data_source_owner: packet_info.data_source_owner.clone(),
            data_packet_classification: packet_info.data_packet_classification.clone(),
            data_packet_accm: packet_info.data_packet_accm.clone(),
            data_model_version: packet_info.data_model_version.clone(),
            application_version: packet_info.application_version.clone(),
        }
    }
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

pub fn get_packet_metadata() -> PacketMetadata {
    // get application_version from Cargo.toml
    let application_version = option_env!("CARGO_PKG_VERSION").map(|s| s.to_string());

    PacketMetadata {
        data_source_geo_location: get_env_or_default("DATA_SOURCE_GEO_LOCATION", None),
        data_source_network_name: get_env_or_default("DATA_SOURCE_NETWORK_NAME", None),
        data_source_system_type: get_env_or_default("DATA_SOURCE_SYSTEM_TYPE", None),
        data_source_owner: get_env_or_default("DATA_SOURCE_OWNER", None),
        data_packet_classification: get_env_or_default("DATA_PACKET_CLASSIFICATION", None),
        data_packet_accm: get_env_or_default("DATA_PACKET_ACCM", None),
        data_model_version: get_env_or_default("FIRST_BITE_DATA_MODEL_VERSION", None),
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
    // get application_version from Cargo.toml
    // this gets the version of the current library ("datamodels"")
    let datamodels_version = option_env!("CARGO_PKG_VERSION").map(|s| s.to_string());

    LogMetadata {
        data_source_geo_location: get_env_or_default("DATA_SOURCE_GEO_LOCATION", None),
        data_source_network_name: get_env_or_default("DATA_SOURCE_NETWORK_NAME", None),
        data_source_system_type: get_env_or_default("DATA_SOURCE_SYSTEM_TYPE", None),
        data_source_owner: get_env_or_default("DATA_SOURCE_OWNER", None),
        data_model_version: get_env_or_default(
            "FIRST_BITE_DATA_MODEL_VERSION",
            datamodels_version.as_deref(),
        ),
        application_version,
    }
}
