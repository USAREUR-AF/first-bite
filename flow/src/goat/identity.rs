//! The shapes goat gives to identities: the slot UUID, the org prefix, the
//! slot alias and the mesh peer name. `docs/goat/contract.md` cites where
//! goat defines each.

use super::GoatError;

/// Top-level key classes goat reserves; none can be an org prefix or an alias.
const RESERVED_ROOTS: [&str; 20] = [
    "data",
    "services",
    "health",
    "config",
    "cmd",
    "attest",
    "ops",
    "agents",
    "release",
    "task",
    "ack",
    "telemetry",
    "volatile",
    "system",
    "community",
    "loadtest",
    "keys",
    "from",
    "shared",
    "hello",
];

/// What the fabric knows a publisher as, and so what it may write under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// A slot of a slot-pool goatnet: the certificate common name and the
    /// key prefix are both this 32-hex UUID.
    Slot(String),
    /// An org of a per-org goatnet: the key prefix is `prefix`, the
    /// certificate common name `prefix` followed by the goatnet's suffix.
    Org { prefix: String, common_name: String },
}

impl Principal {
    /// First chunk of every key the principal may write.
    pub fn prefix(&self) -> &str {
        match self {
            Principal::Slot(uuid) => uuid,
            Principal::Org { prefix, .. } => prefix,
        }
    }

    /// The common name the certificate must carry.
    pub fn common_name(&self) -> &str {
        match self {
            Principal::Slot(uuid) => uuid,
            Principal::Org { common_name, .. } => common_name,
        }
    }

    /// The value for the Zenoh session `id` that pins it to a slot UUID.
    /// A Zenoh identifier is minimal lower-case hexadecimal, so leading zeros
    /// are removed. An org has no such identifier.
    pub fn session_id(&self) -> Option<String> {
        match self {
            Principal::Slot(uuid) => {
                let trimmed = uuid.trim_start_matches('0');
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Principal::Org { .. } => None,
        }
    }
}

/// True for 32 lower-case hexadecimal digits: the shape of a slot UUID.
pub fn is_slot_uuid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Check a configured slot UUID. The error says how to write the value when
/// it is a UUID in another spelling.
pub fn check_slot_uuid(value: &str) -> Result<(), GoatError> {
    if is_slot_uuid(value) {
        return Ok(());
    }
    let compact: String = value
        .chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let fix = if is_slot_uuid(&compact) {
        format!("write it as {compact}")
    } else {
        format!(
            "it has {} characters; copy the value from the name of the certificate file in the onboarding archive (<uuid>-cert.pem)",
            value.chars().count()
        )
    };
    Err(GoatError::Uuid {
        value: value.to_string(),
        fix,
    })
}

/// goat's shape for an org prefix and for a slot alias: `[a-z0-9][a-z0-9-]{1,62}`,
/// not shaped like a slot UUID, not a reserved top-level class.
fn check_name(field: &'static str, value: &str) -> Result<(), GoatError> {
    let bytes = value.as_bytes();
    let shaped = (2..=63).contains(&bytes.len())
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    let problem = if !shaped {
        "must be 2 to 63 characters of a-z, 0-9 and -, not starting with -"
    } else if is_slot_uuid(value) {
        "is shaped like a slot UUID; a UUID goes in identity.slot"
    } else if RESERVED_ROOTS.contains(&value) {
        "is a key class goat reserves"
    } else {
        return Ok(());
    };
    Err(GoatError::Name {
        field,
        value: value.to_string(),
        problem,
    })
}

pub fn check_org_prefix(value: &str) -> Result<(), GoatError> {
    check_name("identity.org", value)
}

pub fn check_alias(value: &str) -> Result<(), GoatError> {
    check_name("identity.alias", value)
}

/// The fields of a mesh peer name, `peer-<handle>[-<org_short_tag>]-<site>-<date>[-<hostname>]`.
/// The handle and the tag may themselves contain `-`, so they come back joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerName {
    /// `<handle>[-<org_short_tag>]`.
    pub principal: String,
    pub site: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub hostname: Option<String>,
}

const PEER_PREFIX: &str = "peer-";

/// Parse a mesh peer name. The date is the anchor: the chunk before it is the
/// site, everything before that the principal, everything after the hostname.
pub fn parse_peer_name(name: &str) -> Result<PeerName, GoatError> {
    let fail = |problem: &'static str| GoatError::PeerName {
        value: name.to_string(),
        problem,
    };
    let rest = name
        .strip_prefix(PEER_PREFIX)
        .ok_or_else(|| fail("it does not start with peer-"))?;
    let chunks: Vec<&str> = rest.split('-').collect();
    let is_number = |chunk: &str, digits: usize| {
        chunk.len() == digits && chunk.bytes().all(|b| b.is_ascii_digit())
    };
    let date_at = (0..chunks.len().saturating_sub(2))
        .find(|&i| {
            is_number(chunks[i], 4) && is_number(chunks[i + 1], 2) && is_number(chunks[i + 2], 2)
        })
        .ok_or_else(|| fail("it has no enrolment date (YYYY-MM-DD)"))?;
    if date_at < 2 {
        return Err(fail("a handle and a site must come before the date"));
    }
    if chunks[..date_at].iter().any(|chunk| chunk.is_empty()) {
        return Err(fail("it has an empty field (a doubled -)"));
    }
    let hostname = chunks[date_at + 3..].join("-");
    Ok(PeerName {
        principal: chunks[..date_at - 1].join("-"),
        site: chunks[date_at - 1].to_string(),
        date: chunks[date_at..date_at + 3].join("-"),
        hostname: (!hostname.is_empty()).then_some(hostname),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: &str = "e7a9c75b532e3d434bb09fefd3ce1ec7";

    #[test]
    fn slot_uuids_are_32_lower_case_hex() {
        assert!(check_slot_uuid(SLOT).is_ok());
        for wrong in ["", "e7a9c75b", &SLOT.to_uppercase(), &format!("{SLOT}0")] {
            assert!(check_slot_uuid(wrong).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn a_uuid_in_another_spelling_is_refused_with_the_right_spelling() {
        let error = check_slot_uuid("E7A9C75B-532E-3D43-4BB0-9FEFD3CE1EC7").unwrap_err();
        assert_eq!(
            error.to_string(),
            "identity.slot \"E7A9C75B-532E-3D43-4BB0-9FEFD3CE1EC7\" is not a slot UUID \
             (32 lower-case hexadecimal digits): write it as e7a9c75b532e3d434bb09fefd3ce1ec7"
        );
        let error = check_slot_uuid("alpha-operator").unwrap_err().to_string();
        assert!(error.contains("it has 14 characters"), "{error}");
    }

    #[test]
    fn org_prefixes_and_aliases_follow_goats_shape() {
        assert!(check_org_prefix("example-vendor").is_ok());
        assert!(check_alias("alpha-operator").is_ok());
        for wrong in [
            "a",
            "-vendor",
            "Example",
            "ex/ample",
            "ops",
            "telemetry",
            SLOT,
        ] {
            assert!(check_org_prefix(wrong).is_err(), "{wrong:?}");
            assert!(check_alias(wrong).is_err(), "{wrong:?}");
        }
        assert_eq!(
            check_alias(SLOT).unwrap_err().to_string(),
            format!("identity.alias \"{SLOT}\" is shaped like a slot UUID; a UUID goes in identity.slot")
        );
    }

    #[test]
    fn the_session_id_is_the_uuid_without_leading_zeros() {
        assert_eq!(
            Principal::Slot(SLOT.to_string()).session_id().as_deref(),
            Some(SLOT)
        );
        assert_eq!(
            Principal::Slot("000a9c75b532e3d434bb09fefd3ce1ec".to_string())
                .session_id()
                .as_deref(),
            Some("a9c75b532e3d434bb09fefd3ce1ec")
        );
        let org = Principal::Org {
            prefix: "example-vendor".to_string(),
            common_name: "example-vendor-trial-peer".to_string(),
        };
        assert_eq!(org.session_id(), None);
        assert_eq!(org.prefix(), "example-vendor");
        assert_eq!(org.common_name(), "example-vendor-trial-peer");
    }

    /// Names from the conformance vectors of goat's
    /// `ops/identity/peer-name-grammar.yaml`.
    #[test]
    fn peer_names_parse_around_the_date() {
        let parsed = parse_peer_name("peer-maintainer-zagros-2026-06-10-mac").unwrap();
        assert_eq!(
            parsed,
            PeerName {
                principal: "maintainer".to_string(),
                site: "zagros".to_string(),
                date: "2026-06-10".to_string(),
                hostname: Some("mac".to_string()),
            }
        );
        let parsed = parse_peer_name("peer-maintainer-acme-corp-nova-2026-05-17").unwrap();
        assert_eq!(parsed.principal, "maintainer-acme-corp");
        assert_eq!(parsed.site, "nova");
        assert_eq!(parsed.hostname, None);
    }

    #[test]
    fn names_outside_the_grammar_say_what_is_wrong() {
        let problem = |name: &str| parse_peer_name(name).unwrap_err().to_string();
        assert!(problem("thor-a").contains("does not start with peer-"));
        assert!(problem("Peer-maintainer-zagros-2026-06-10").contains("does not start with peer-"));
        assert!(problem("peer-maintainer-zagros").contains("no enrolment date"));
        assert!(problem("peer-zagros-2026-06-10").contains("a handle and a site"));
        assert!(problem("peer-maintainer--zagros-2026-06-10").contains("empty field"));
    }
}
