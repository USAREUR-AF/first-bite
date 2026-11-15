//! Unwrapping mirrored traffic that arrives inside a tunnel.
//!
//! A mirror port on the same switch delivers frames as they were. A remote
//! mirror delivers each frame wrapped in an outer IP packet: in GRE (plain
//! GRE-tunnelled mirroring and ERSPAN) or in VXLAN (traffic mirroring in
//! virtualised networks). The record must describe the mirrored packet, not
//! the tunnel, so the pipeline unwraps these before it looks for a message.

use crate::source::LinkType;

/// What a tunnel header was found to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tunnel {
    /// GRE carrying an Ethernet frame (protocol type 0x6558).
    GreEthernet,
    /// GRE carrying an IPv4 or IPv6 packet.
    GreIp,
    /// ERSPAN type I: GRE protocol 0x88BE with no ERSPAN header.
    ErspanI,
    /// ERSPAN type II: GRE protocol 0x88BE with an 8-byte ERSPAN header.
    ErspanII,
    /// ERSPAN type III: GRE protocol 0x22EB with a 12-byte header and an
    /// optional 8-byte sub-header.
    ErspanIII,
    /// VXLAN: an 8-byte header after UDP, carrying an Ethernet frame.
    Vxlan,
}

const GRE_CHECKSUM: u8 = 0x80;
const GRE_ROUTING: u8 = 0x40;
const GRE_KEY: u8 = 0x20;
const GRE_SEQUENCE: u8 = 0x10;

const PROTO_TRANSPARENT_ETHERNET: u16 = 0x6558;
const PROTO_ERSPAN_I_II: u16 = 0x88BE;
const PROTO_ERSPAN_III: u16 = 0x22EB;
const PROTO_IPV4: u16 = 0x0800;
const PROTO_IPV6: u16 = 0x86DD;

/// The packet inside a GRE header, given the bytes that follow the outer IP
/// header. `None` when the header is not one this function understands or the
/// bytes are too short.
pub fn gre_inner(gre: &[u8]) -> Option<(Tunnel, LinkType, &[u8])> {
    let flags = *gre.first()?;
    let version = gre.get(1)? & 0x07;
    let protocol = u16::from_be_bytes([*gre.get(2)?, *gre.get(3)?]);
    // Version 1 is PPTP's variant; source routing was deprecated by RFC 2784.
    if version != 0 || flags & GRE_ROUTING != 0 {
        return None;
    }
    let mut offset = 4;
    if flags & GRE_CHECKSUM != 0 {
        offset += 4;
    }
    if flags & GRE_KEY != 0 {
        offset += 4;
    }
    let has_sequence = flags & GRE_SEQUENCE != 0;
    if has_sequence {
        offset += 4;
    }
    let rest = gre.get(offset..)?;
    match protocol {
        PROTO_TRANSPARENT_ETHERNET => Some((Tunnel::GreEthernet, LinkType::Ethernet, rest)),
        PROTO_IPV4 | PROTO_IPV6 => Some((Tunnel::GreIp, LinkType::RawIp, rest)),
        // Type II always carries a GRE sequence number; type I never does and
        // has no ERSPAN header at all.
        PROTO_ERSPAN_I_II if has_sequence => {
            Some((Tunnel::ErspanII, LinkType::Ethernet, rest.get(8..)?))
        }
        PROTO_ERSPAN_I_II => Some((Tunnel::ErspanI, LinkType::Ethernet, rest)),
        PROTO_ERSPAN_III => {
            let header = rest.get(..12)?;
            // The O bit, the last bit of the header, announces a sub-header.
            let sub_header = if header[11] & 0x01 != 0 { 8 } else { 0 };
            Some((
                Tunnel::ErspanIII,
                LinkType::Ethernet,
                rest.get(12 + sub_header..)?,
            ))
        }
        _ => None,
    }
}

/// The Ethernet frame inside a VXLAN header, given the UDP payload. `None`
/// unless the header's "VNI present" flag is set.
pub fn vxlan_inner(udp_payload: &[u8]) -> Option<(Tunnel, LinkType, &[u8])> {
    let header = udp_payload.get(..8)?;
    if header[0] & 0x08 == 0 {
        return None;
    }
    Some((Tunnel::Vxlan, LinkType::Ethernet, &udp_payload[8..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: &[u8] = &[0xAA, 0xBB, 0xCC];

    fn with(header: &[u8]) -> Vec<u8> {
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(INNER);
        bytes
    }

    #[test]
    fn plain_gre_carrying_ethernet() {
        let bytes = with(&[0x00, 0x00, 0x65, 0x58]);
        assert_eq!(
            gre_inner(&bytes),
            Some((Tunnel::GreEthernet, LinkType::Ethernet, INNER))
        );
    }

    #[test]
    fn gre_with_key_and_checksum_carrying_ip() {
        let bytes = with(&[0xA0, 0x00, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 7]);
        assert_eq!(
            gre_inner(&bytes),
            Some((Tunnel::GreIp, LinkType::RawIp, INNER))
        );
    }

    #[test]
    fn erspan_type_i_has_no_erspan_header() {
        let bytes = with(&[0x00, 0x00, 0x88, 0xBE]);
        assert_eq!(
            gre_inner(&bytes),
            Some((Tunnel::ErspanI, LinkType::Ethernet, INNER))
        );
    }

    #[test]
    fn erspan_type_ii_skips_sequence_and_eight_byte_header() {
        let mut header = vec![0x10, 0x00, 0x88, 0xBE, 0, 0, 0, 1];
        header.extend_from_slice(&[0x10, 0x64, 0x00, 0x01, 0, 0, 0, 0]);
        let bytes = with(&header);
        assert_eq!(
            gre_inner(&bytes),
            Some((Tunnel::ErspanII, LinkType::Ethernet, INNER))
        );
    }

    #[test]
    fn erspan_type_iii_with_and_without_sub_header() {
        let mut header = vec![0x10, 0x00, 0x22, 0xEB, 0, 0, 0, 1];
        header.extend_from_slice(&[0x20, 0x64, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            gre_inner(&with(&header)),
            Some((Tunnel::ErspanIII, LinkType::Ethernet, INNER))
        );
        let last = header.len() - 1;
        header[last] |= 0x01;
        header.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            gre_inner(&with(&header)),
            Some((Tunnel::ErspanIII, LinkType::Ethernet, INNER))
        );
    }

    #[test]
    fn unknown_or_truncated_gre_is_not_unwrapped() {
        assert_eq!(gre_inner(&[0x00, 0x00, 0x12, 0x34, 1, 2]), None);
        assert_eq!(gre_inner(&[0x00, 0x01, 0x65, 0x58, 1, 2]), None); // version 1
        assert_eq!(gre_inner(&[0x40, 0x00, 0x65, 0x58, 1, 2]), None); // routing
        assert_eq!(gre_inner(&[0x10, 0x00, 0x88, 0xBE, 0, 0, 0, 1, 0, 0]), None);
        assert_eq!(gre_inner(&[0x00, 0x00]), None);
        assert_eq!(gre_inner(&[]), None);
    }

    #[test]
    fn vxlan_needs_the_vni_flag() {
        let bytes = with(&[0x08, 0, 0, 0, 0, 0, 0x2A, 0]);
        assert_eq!(
            vxlan_inner(&bytes),
            Some((Tunnel::Vxlan, LinkType::Ethernet, INNER))
        );
        assert_eq!(vxlan_inner(&with(&[0x00, 0, 0, 0, 0, 0, 0x2A, 0])), None);
        assert_eq!(vxlan_inner(&[0x08, 0, 0]), None);
    }
}
