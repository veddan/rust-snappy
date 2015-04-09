use std::default::Default;
use std::io::{BufRead, Write};
use std::io;
use std::collections::HashMap;
use util::native_to_le32;

const MIN_COPY_LEN: u32 = 4;

pub trait SnappyRead : BufRead {
    /// Returns the total number of bytes left to be read.
    fn available(&self) -> u64;
}

impl <'a> SnappyRead for io::Cursor<&'a [u8]> {
    fn available(&self) -> u64 { self.get_ref().len() as u64 - self.position() }
}

impl <'a> SnappyRead for io::Cursor<&'a mut [u8]> {
    fn available(&self) -> u64 { self.get_ref().len() as u64 - self.position() }
}

impl <'a> SnappyRead for io::Cursor<Vec<u8>> {
    fn available(&self) -> u64 { self.get_ref().len() as u64 - self.position() }
}


pub struct CompressorOptions {
    block_size: u32,
}

impl Default for CompressorOptions {
    fn default() -> CompressorOptions {
        CompressorOptions {
            block_size: 65536,  // Seems to be a popular buffer size
        }
    }
}

#[inline(never)]
pub fn compress<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W) -> io::Result<()> {
    compress_with_options(inp, out, &Default::default())
}

pub fn compress_with_options<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W,
                                                   options: &CompressorOptions) -> io::Result<()> {
    assert!(inp.available() <= ::std::u32::MAX as u64);
    let uncompressed_length = inp.available() as u32;
    try!(write_varint(out, uncompressed_length));
    let mut dict = HashMap::new();
    loop {
        let mut len;
        {
            let buf = match inp.fill_buf() {
                Ok(b) if b.len() == 0 => return Ok(()),
                Ok(b)  => b,
                Err(e) => return Err(e)
            };
            len = buf.len();
            for chunk in buf.chunks(options.block_size as usize) {
                try!(compress_block(chunk, out, &mut dict));
                dict.clear();
            }
        }
        inp.consume(len);
    }
}

// TODO Consider using [u8; MIN_COPY_LEN] instead of &[u8] in `dict`
fn compress_block<W: Write>(block: &[u8], out: &mut W, dict: &mut HashMap<&[u8], Vec<u32>>) -> io::Result<()> {
    // TODO: Implement properly
    let max_literal_len = ::std::u32::MAX as usize;
    for chunk in block.chunks(max_literal_len) {
        try!(out.write_all(&[0xFC]));
        try!(write_u32_le(out, (chunk.len() - 1) as u32));
        try!(out.write_all(chunk));
    }
    Ok(())
}

fn write_u32_le<W: Write>(out: &mut W, n: u32) -> io::Result<()> {
    let le = native_to_le32(n);
    let ptr = &le as *const u32 as *const u8;
    let s = unsafe { ::std::slice::from_raw_parts(ptr, 4) };
    out.write_all(s)
}

fn write_varint<W: Write>(out: &mut W, n: u32) -> io::Result<()> {
    let r = 128;
    let mut ds = [0, 0, 0, 0, 0];
    let nbytes = if n < (1 << 7) {
        ds[0] = n as u8;
        1
    } else if n < (1 << 14) {
        ds[0] = (n | r) as u8;
        ds[1] = (n >> 7) as u8;
        2
    } else if n < (1 << 21) {
        ds[0] = (n | r) as u8;
        ds[1] = ((n >> 7) | r) as u8;
        ds[2] = (n >> 14) as u8;
        3
    } else if n < (1 << 28) {
        ds[0] = (n | r) as u8;
        ds[1] = ((n >> 7) | r) as u8;
        ds[2] = ((n >> 14) | r) as u8;
        ds[3] = (n >> 21) as u8;
        4
    } else {
        ds[0] = (n | r) as u8;
        ds[1] = ((n >> 7) | r) as u8;
        ds[2] = ((n >> 14) | r) as u8;
        ds[3] = ((n >> 21) |r) as u8;
        ds[4] = (n >> 28) as u8;
        5
    };
    try!(out.write(&ds[..nbytes]));
    Ok(())
}

#[cfg(test)]
mod test {
    use super::write_varint;

    #[test]
    fn test_write_varint_short() {
        let mut v = Vec::new();
        write_varint(&mut v, 64).unwrap();
        assert_eq!(&v[..], &[64])
    }

    #[test]
    fn test_write_varint_long() {
        let mut v = Vec::new();
        write_varint(&mut v, 2097150).unwrap();
        assert_eq!(&v[..], &[0xFE, 0xFF, 0x7F])
    }
}