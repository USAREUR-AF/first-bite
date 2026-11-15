//! One TLS handshake with a sink's endpoint, to read what was negotiated.
//!
//! The probe uses the TLS configuration the sink itself would use: the same
//! provider, versions, groups, trusted authorities and client certificate.
//! It sends no record.

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection};
use serde::Serialize;
use std::fmt;
use std::io::{ErrorKind, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the probe waits, once its side of the handshake is done, for a
/// server that rejects the client's certificate to say so.
const VERDICT_WAIT: Duration = Duration::from_millis(1500);

/// An endpoint a probe can handshake with.
#[derive(Clone)]
pub struct ProbeTarget {
    /// The subscriber's name.
    pub sink: String,
    /// The endpoint as the posture report shows it.
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    pub(crate) config: Arc<ClientConfig>,
}

impl fmt::Debug for ProbeTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProbeTarget")
            .field("sink", &self.sink)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// What one handshake negotiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Negotiated {
    pub sink: String,
    pub endpoint: String,
    pub tls_version: String,
    pub cipher_suite: String,
    pub key_exchange_group: String,
    /// Whether the handshake was performed by a FIPS-mode configuration.
    pub fips: bool,
}

impl fmt::Display for Negotiated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "probe {} {}: negotiated {}, cipher suite {}, key exchange group {}, FIPS mode {}",
            self.sink,
            self.endpoint,
            self.tls_version,
            self.cipher_suite,
            self.key_exchange_group,
            if self.fips { "yes" } else { "no" }
        )
    }
}

/// Why a probe got no handshake.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("probe {sink} {endpoint}: {reason}")]
    Failed {
        sink: String,
        endpoint: String,
        reason: String,
    },
}

/// Connect, complete one TLS handshake, report it and close.
pub fn probe(target: &ProbeTarget) -> Result<Negotiated, ProbeError> {
    let failed = |reason: String| ProbeError::Failed {
        sink: target.sink.clone(),
        endpoint: target.endpoint.clone(),
        reason,
    };
    let name = ServerName::try_from(target.host.clone())
        .map_err(|e| failed(format!("{:?} is not a server name: {e}", target.host)))?;
    let address = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|e| failed(format!("cannot resolve {}: {e}", target.host)))?
        .next()
        .ok_or_else(|| failed(format!("{} resolves to no address", target.host)))?;
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|e| failed(format!("cannot connect to {address}: {e}")))?;
    stream
        .set_read_timeout(Some(CONNECT_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CONNECT_TIMEOUT)))
        .map_err(|e| failed(e.to_string()))?;
    let mut connection = ClientConnection::new(Arc::clone(&target.config), name)
        .map_err(|e| failed(format!("cannot start TLS: {e}")))?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut stream)
            .map_err(|e| failed(format!("handshake failed: {e}")))?;
    }
    // In TLS 1.3 the client finishes before the server has judged the
    // client's certificate; a rejection arrives as an alert just after.
    stream
        .set_read_timeout(Some(VERDICT_WAIT))
        .map_err(|e| failed(e.to_string()))?;
    let mut tls = rustls::Stream::new(&mut connection, &mut stream);
    let mut byte = [0u8; 1];
    match tls.read(&mut byte) {
        Ok(_) => {}
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {}
        Err(e) => return Err(failed(format!("the peer refused the connection: {e}"))),
    }
    let negotiated = Negotiated {
        sink: target.sink.clone(),
        endpoint: target.endpoint.clone(),
        tls_version: connection
            .protocol_version()
            .map_or_else(|| "unknown".to_string(), |version| format!("{version:?}")),
        cipher_suite: connection.negotiated_cipher_suite().map_or_else(
            || "unknown".to_string(),
            |suite| format!("{:?}", suite.suite()),
        ),
        key_exchange_group: connection.negotiated_key_exchange_group().map_or_else(
            || "unknown".to_string(),
            |group| format!("{:?}", group.name()),
        ),
        fips: target.config.fips(),
    };
    connection.send_close_notify();
    let _ = connection.complete_io(&mut stream);
    Ok(negotiated)
}
