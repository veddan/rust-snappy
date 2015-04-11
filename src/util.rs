#[inline(always)]
pub fn native_to_le32(n: u32) -> u32 {
    if cfg!(target_endian = "little") {
        n
    } else {
        bswap32(n)
    }
}

#[inline(always)]
pub fn native_to_le16(n: u16) -> u16 {
    if cfg!(target_endian = "little") {
        n
    } else {
        bswap16(n)
    }
}

pub fn next_power_of_2(n: u32) -> u32 {
    let mut v = n.wrapping_sub(1);
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v.wrapping_add(1)
}

// Rust's built-in bswap* functions are not stable yet. Until they are, I'll provide my own.
fn bswap16(n: u16) -> u16 {
    (n >> 8) | (n & 0xFF) << 8
}

fn bswap32(n: u32) -> u32 {
    (n & 0xFF) << 24 | (n & 0xFF00) << 8 | (n & 0xFF0000) >> 8 | (n >> 24) & 0xFF
}

// Same goes for find-least-significant-bit-not-set.

/// Returns the index of the least significant set bit in `n`.
/// ...10100100000 => 5
/// If `n` is 0 the return value is undefined.
#[allow(dead_code)]
pub fn find_lsb_set64(n: u64) -> u32 {
    // http://chessprogramming.wikispaces.com/BitScan
    let magic = &[
        0,  47,  1, 56, 48, 27,  2, 60,
        57, 49, 41, 37, 28, 16,  3, 61,
        54, 58, 35, 52, 50, 42, 21, 44,
        38, 32, 29, 23, 17, 11,  4, 62,
        46, 55, 26, 59, 40, 36, 15, 53,
        34, 51, 20, 43, 31, 22, 10, 45,
        25, 39, 14, 33, 19, 30,  9, 24,
        13, 18,  8, 12,  7,  6,  5, 63
    ];
    let debruijn = 0x03F79D71B4CB0A89;
    magic[((n ^ n.wrapping_sub(1)).wrapping_mul(debruijn) >> 58) as usize]
}

#[allow(dead_code)]
pub fn find_lsb_set32(n: u32) -> u32 {
    let magic = &[
        0,  1,  28, 2,  29, 14, 24, 3,
        30, 22, 20, 15, 25, 17, 4,  8,
        31, 27, 13, 23, 21, 19, 16, 7,
        26, 12, 18, 6,  11, 5,  10, 9
    ];
    let debruijn = 0x077CB531;
    magic[((n & (!n).wrapping_add(1)).wrapping_mul(debruijn) >> 27) as usize]
}

#[cfg(test)]
mod test {
    use super::{bswap16, bswap32, find_lsb_set64, find_lsb_set32};

    #[test]
    fn test_bswap16() {
        assert_eq!(bswap16(0x1234), 0x3412);
    }

    #[test]
    fn test_bswap32() {
        assert_eq!(bswap32(0x12345678), 0x78563412);
    }

    #[test]
    fn test_find_lsb_set64() {
        assert_eq!(find_lsb_set64(0b10100100000), 5);
        assert_eq!(find_lsb_set64(0b10100100010), 1);
        assert_eq!(find_lsb_set64(0x880000000), 31);
    }

    #[test]
    fn test_find_lsb_set32() {
        assert_eq!(find_lsb_set32(0b10100100000), 5);
        assert_eq!(find_lsb_set32(0b10100100010), 1);
        assert_eq!(find_lsb_set32(0xC0000000), 30);
    }
}