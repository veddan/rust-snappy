use std::default::Default;
use std::io::{BufRead, Write};
use std::io;
use std::cmp;
use std::collections::HashMap;
use util::{native_to_le16, native_to_le32};

const LITERAL: u8 = 0;
const COPY_1_BYTE: u8 = 1;
const COPY_2_BYTE: u8 = 2;
const COPY_3_BYTE: u8 = 3;

const MIN_COPY_LEN: usize = 4;
const MAX_COPY_LEN: usize = 64;

const BLOCK_MARGIN: usize = 16;

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

struct Dict {
    table: HashMap<[u8; MIN_COPY_LEN], Vec<u32>>
}

impl Dict {
    fn new(capacity: usize) -> Dict {
        Dict {
            table: HashMap::with_capacity(capacity)
        }
    }

    #[cfg(not(debug_assertions))]
    fn clear(&mut self) {
        self.table.clear();
    }

    #[cfg(debug_assertions)]
    fn clear(&mut self) {
        self.dump_sizes();
        self.table.clear();
    }

    #[cfg(debug_assertions)]
    fn dump_sizes(&self) {
        let mut histogram = HashMap::new();
        for positions in self.table.values() {
            *histogram.entry(positions.len()).or_insert(0) += 1;
        }
        for (size, count) in histogram.iter() {
            println!("{}:\t{}", size, count);
        }
        println!("len = {}, capacity = {}", self.table.len(), self.table.capacity());
    }

    fn find_best_match_or_add(&mut self, block: &[u8], start: usize) -> Option<(u32, u8)> {
        let prefix = &block[start..start + MIN_COPY_LEN];
        let key = [prefix[0], prefix[1], prefix[2],prefix[3]];
        let mut found = true;
        let positions = self.table.entry(key).or_insert_with(|| { found = false; Vec::with_capacity(3) });
        if !found {
            positions.push(start as u32);
            positions.sort_by(|a, b| b.cmp(a));
            return None;
        }

        assert!(positions.len() > 0);

        let mut posit = positions.iter();
        let mut best_pos = *posit.next().unwrap();
        let mut best_len = common_prefix_length(&block[best_pos as usize..start], &block[start..]);
        if best_len < MAX_COPY_LEN as u8 {
            for &pos in posit {
                let len = common_prefix_length(&block[best_pos as usize..start], &block[start..]);
                if len > best_len {
                    best_pos = pos;
                    best_len = len;
                    if len == MAX_COPY_LEN as u8 { break; }
                }
            }
        }
        return Some((best_pos, best_len));
    }
}


pub fn compress<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W) -> io::Result<()> {
    compress_with_options(inp, out, &Default::default())
}

#[inline(never)]
pub fn compress_with_options<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W,
                                                   options: &CompressorOptions) -> io::Result<()> {
    assert!(inp.available() <= ::std::u32::MAX as u64);
    let uncompressed_length = inp.available() as u32;
    try!(write_varint(out, uncompressed_length));
    let mut dict = Dict::new(32);  // TODO Figure out capacity
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
                try!(compress_block(chunk, out, &mut dict, options));
                dict.clear();
            }
        }
        inp.consume(len);
    }
}

fn compress_block<W: Write>(block: &[u8], out: &mut W, dict: &mut Dict, options: &CompressorOptions)
                             -> io::Result<()> {
    if block.len() < BLOCK_MARGIN {  // Too short to bother with copies.
        return emit_literal(out, block);
    }
    let imax = block.len() - BLOCK_MARGIN;
    let mut i = 0;
    let mut literal_start = 0;
    'outer: while i < imax {
        let mut copy_offset;
        let mut copy_len;
        loop {
            match dict.find_best_match_or_add(block, i) {
                None => {},
                Some((pos, len)) => {
                    copy_offset = i as u32 - pos;
                    copy_len = len;
                    break;
                }
            }
            i += 1;
            if i >= imax { break 'outer; }
        }

        //println!("literal_start = {}, i = {}", literal_start, i);
        try!(emit_literal(out, &block[literal_start..i]));

        loop {
            i += copy_len as usize;
            try!(emit_copy(out, copy_offset, copy_len));
            literal_start = i;
            if i >= imax { break 'outer; }
            match dict.find_best_match_or_add(block, i) {
                None => break,
                Some((pos, len)) => {
                    copy_offset = i as u32 - pos;
                    copy_len = len;
                }
            }
        }
        i += 1;
    }
    if literal_start < block.len() {
        emit_literal(out, &block[literal_start..])
    } else {
        Ok(())
    }
}

fn emit_copy<W: Write>(out: &mut W, offset: u32, len: u8) -> io::Result<()> {
    assert!(len > 0);
    assert!(len <= MAX_COPY_LEN as u8);
    //println!("<copy len={} offset={}>", len, offset);
    if len <= 11 && offset <= 2047 {
        let n = len - 4;
        let tag = (n << 2) | COPY_1_BYTE | ((offset >> 3) & 0xE0) as u8;
        let low_len = (offset & 0xFF) as u8;
        try!(out.write(&[tag, low_len]));
    } else if offset <= 65535 {
        let n = len - 1;
        let tag = (n << 2) | COPY_2_BYTE;
        try!(out.write(&[tag]));
        try!(write_u16_le(out, offset as u16));
    } else {
        panic!("<copy (4-byte) len={} offset={}>", len, offset)
    }
    Ok(())
}

fn emit_literal<W: Write>(out: &mut W, literal: &[u8]) -> io::Result<()> {
    assert!(literal.len() < ::std::u32::MAX as usize);
    //println!("<literal len={}>", literal.len());
    let len = literal.len() - 1;
    if len < 60 {
        let tag = ((len as u8) << 2) | LITERAL;
        try!(out.write(&[tag]));
        try!(out.write_all(literal));
    } else {
        let mut ds = [0, 0, 0, 0];
        let mut n = len;
        let mut count = 0;
        while n > 0 {
            ds[count] = (n & 0xFF) as u8;
            n >>= 8;
            count += 1;
        }
        let tag = (((59 + count) as u8) << 2) | LITERAL;
        try!(out.write(&[tag]));
        try!(out.write(&ds[..count]));
        try!(out.write_all(literal));
    }
    Ok(())
}

fn common_prefix_length<T: Eq>(a: &[T], b: &[T]) -> u8 {
    let n = cmp::min(a.len(), b.len());
    let mut i = 0;
    while i < n && a[i] == b[i] && i <  MAX_COPY_LEN { i += 1; }
    return i as u8;
}

fn write_u32_le<W: Write>(out: &mut W, n: u32) -> io::Result<()> {
    let le = native_to_le32(n);
    let ptr = &le as *const u32 as *const u8;
    let s = unsafe { ::std::slice::from_raw_parts(ptr, 4) };
    out.write_all(s)
}

fn write_u16_le<W: Write>(out: &mut W, n: u16) -> io::Result<()> {
    let le = native_to_le16(n);
    let ptr = &le as *const u16 as *const u8;
    let s = unsafe { ::std::slice::from_raw_parts(ptr, 2) };
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
    use super::{write_varint, emit_literal};

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

    #[test]
    fn test_emit_literal_small() {
        let mut out = Vec::new();
        let literal = &[1, 2, 3, 4, 5, 6, 7];
        emit_literal(&mut out, literal).unwrap();
        assert_eq!(out[0], 0b000110_00);
        assert_eq!(&out[1..], literal);
    }

    #[test]
    fn test_emit_literal_medium() {
        let mut out = Vec::new();
        let literal: Vec<u8> = (0..100).collect();
        emit_literal(&mut out, &literal[..]).unwrap();
        assert_eq!(&out[..2], &[0b111100_00, (literal.len() - 1) as u8]);
        assert_eq!(&out[2..], &literal[..]);
    }

    #[ignore]
    #[test]
    fn test_emit_literal_large() {
        let mut out = Vec::new();
        let literal: Vec<u8> = (0..16_777_218).map(|i| (i % 100) as u8).collect();
        emit_literal(&mut out, &literal[..]).unwrap();
        assert_eq!(&out[..5], &[0b111111_00, 0x01, 0x00, 0x00, 0x01]);
        assert_eq!(&out[5..], &literal[..]);
    }
}