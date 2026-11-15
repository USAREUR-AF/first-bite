//! Bit-field extraction from a byte slice.

use crate::schema::BitOrder;

/// Extract `stop - start` bits (1 to 64) as an unsigned integer.
///
/// Returns `None` when the field is wider than 64 bits, empty, or reaches past
/// the end of `data`.
///
/// With [`BitOrder::Msb0`], bit 0 is the most significant bit of byte 0 and the
/// field is read as a big-endian bit string. This is the numbering network
/// protocol documents use.
///
/// With [`BitOrder::Lsb0`], bit 0 is the least significant bit of byte 0 and the
/// field is read from the payload taken as one little-endian integer. This is
/// the numbering of little-endian ("Intel") signals on CAN.
pub fn extract(data: &[u8], start: usize, stop: usize, order: BitOrder) -> Option<u64> {
    if stop <= start || stop - start > 64 || stop > data.len() * 8 {
        return None;
    }
    let width = stop - start;
    let first = start / 8;
    let last = (stop - 1) / 8;
    // At most nine bytes are touched, which fits in a u128.
    let bytes = &data[first..=last];
    let value = match order {
        BitOrder::Msb0 => {
            let acc = bytes.iter().fold(0u128, |acc, &b| (acc << 8) | b as u128);
            let trailing = (last + 1) * 8 - stop;
            acc >> trailing
        }
        BitOrder::Lsb0 => {
            let acc = bytes
                .iter()
                .rev()
                .fold(0u128, |acc, &b| (acc << 8) | b as u128);
            acc >> (start % 8)
        }
    };
    Some((value & mask(width)) as u64)
}

fn mask(width: usize) -> u128 {
    (1u128 << width) - 1
}

/// Interpret the low `width` bits of `raw` as a two's-complement integer.
/// A width of zero has no bits and gives zero.
pub fn sign_extend(raw: u64, width: usize) -> i64 {
    if width >= 64 {
        return raw as i64;
    }
    if width == 0 {
        return 0;
    }
    let shift = 64 - width;
    ((raw << shift) as i64) >> shift
}

/// Reverse the byte order of a value that is `width / 8` whole bytes wide.
/// Bits of a partial byte are dropped; a width of zero gives zero and a width
/// over 64 is taken as 64.
pub fn swap_bytes(raw: u64, width: usize) -> u64 {
    let bits = (width.min(64) / 8) * 8;
    if bits == 0 {
        return 0;
    }
    raw.swap_bytes() >> (64 - bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_aligned_big_endian() {
        let data = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(extract(&data, 0, 8, BitOrder::Msb0), Some(0x12));
        assert_eq!(extract(&data, 8, 24, BitOrder::Msb0), Some(0x3456));
        assert_eq!(extract(&data, 0, 32, BitOrder::Msb0), Some(0x1234_5678));
    }

    #[test]
    fn bits_inside_one_byte() {
        // 0b1011_0010
        let data = [0xB2];
        assert_eq!(extract(&data, 0, 1, BitOrder::Msb0), Some(1));
        assert_eq!(extract(&data, 1, 4, BitOrder::Msb0), Some(0b011));
        assert_eq!(extract(&data, 4, 8, BitOrder::Msb0), Some(0b0010));
    }

    #[test]
    fn bits_across_byte_boundaries() {
        // 0b0000_1111 0b1111_0000
        let data = [0x0F, 0xF0];
        assert_eq!(extract(&data, 4, 12, BitOrder::Msb0), Some(0xFF));
        assert_eq!(extract(&data, 6, 10, BitOrder::Msb0), Some(0b1111));
        assert_eq!(extract(&data, 7, 9, BitOrder::Msb0), Some(0b11));
    }

    #[test]
    fn sixty_four_bits_off_alignment() {
        let data = [0xFF; 9];
        assert_eq!(extract(&data, 3, 67, BitOrder::Msb0), Some(u64::MAX));
    }

    #[test]
    fn out_of_range_is_none() {
        let data = [0u8; 2];
        assert_eq!(extract(&data, 8, 24, BitOrder::Msb0), None);
        assert_eq!(extract(&data, 4, 4, BitOrder::Msb0), None);
        assert_eq!(extract(&data, 8, 4, BitOrder::Msb0), None);
        assert_eq!(extract(&[0u8; 16], 0, 65, BitOrder::Msb0), None);
    }

    #[test]
    fn lsb0_little_endian() {
        // Little-endian integer 0x3412; bits 4..12 are 0x41.
        let data = [0x12, 0x34];
        assert_eq!(extract(&data, 0, 16, BitOrder::Lsb0), Some(0x3412));
        assert_eq!(extract(&data, 4, 12, BitOrder::Lsb0), Some(0x41));
        assert_eq!(extract(&data, 0, 4, BitOrder::Lsb0), Some(0x2));
    }

    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0xFF80_0000, 32), -8_388_608);
        assert_eq!(sign_extend(0b100, 3), -4);
        assert_eq!(sign_extend(u64::MAX, 64), -1);
    }

    #[test]
    fn byte_swap() {
        assert_eq!(swap_bytes(0x1234, 16), 0x3412);
        assert_eq!(swap_bytes(0x1234_5678, 32), 0x7856_3412);
        assert_eq!(swap_bytes(0xAB, 8), 0xAB);
    }

    #[test]
    fn widths_outside_a_field_do_not_overflow_a_shift() {
        assert_eq!(sign_extend(0xFF, 0), 0);
        assert_eq!(sign_extend(1, 1), -1);
        assert_eq!(swap_bytes(0xAB, 0), 0);
        assert_eq!(swap_bytes(0xAB, 7), 0);
        assert_eq!(swap_bytes(0x0102_0304_0506_0708, 64), 0x0807_0605_0403_0201);
        assert_eq!(
            swap_bytes(0x0102_0304_0506_0708, 200),
            0x0807_0605_0403_0201
        );
        assert_eq!(
            extract(&[0xFF; 4], usize::MAX - 3, usize::MAX, BitOrder::Msb0),
            None
        );
        assert_eq!(
            extract(&[0xFF; 4], usize::MAX - 3, usize::MAX, BitOrder::Lsb0),
            None
        );
    }
}
