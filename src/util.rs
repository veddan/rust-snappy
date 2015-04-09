pub fn native_to_le32(n: u32) -> u32 {
    if cfg!(target_endian = "little") {
        n
    } else {
        bswap32(n)
    }
}

pub fn native_to_le16(n: u16) -> u16 {
    if cfg!(target_endian = "little") {
        n
    } else {
        bswap16(n)
    }
}

// Rust's built-in bswap* functions are not stable yet. Until they are, I'll provide my own.

fn bswap16(n: u16) -> u16 {
    (n >> 8) | (n & 0xFF) << 8
}

fn bswap32(n: u32) -> u32 {
    (n & 0xFF) << 24 | (n & 0xFF00) << 8 | (n & 0xFF0000) >> 8 | (n >> 24) & 0xFF
}

#[cfg(test)]
mod test {
    use super::{bswap16, bswap32};

    #[test]
    fn test_bswap16() {
        assert_eq!(bswap16(0x1234), 0x3412);
    }

    #[test]
    fn test_bswap32() {
        assert_eq!(bswap32(0x12345678), 0x78563412);
    }
}