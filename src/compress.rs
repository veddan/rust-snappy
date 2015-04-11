use std::default::Default;
use std::io::{BufReader, BufRead, Write};
use std::io;
use std::cmp;
use std::ptr;
use std::slice::Iter;
use std::fs::File;
use util::{native_to_le16, native_to_le32, next_power_of_2};
use util;

const LITERAL: u8 = 0;
const COPY_1_BYTE: u8 = 1;
const COPY_2_BYTE: u8 = 2;

const MIN_COPY_LEN: u16 = 4;
const MAX_COPY_LEN: u16 = 64;

const BLOCK_MARGIN: usize = 16;

pub const MAX_BLOCK_SIZE: usize = ::std::u16::MAX as usize;

/// Maximum number of positions stored for one prefix. Must not be 0.
/// Larger values leads to better compression, but worsens compression speed and memory usage.
const MAX_CHAIN_LEN: u8 = 3;

pub trait SnappyRead : BufRead {
    /// Returns the total number of bytes left to be read.
    fn available(&self) -> io::Result<u64>;
}

impl <'a> SnappyRead for io::Cursor<&'a [u8]> {
    fn available(&self) -> io::Result<u64> { Ok(self.get_ref().len() as u64 - self.position()) }
}

impl <'a> SnappyRead for io::Cursor<&'a mut [u8]> {
    fn available(&self) -> io::Result<u64>{ Ok(self.get_ref().len() as u64 - self.position()) }
}

impl SnappyRead for io::Cursor<Vec<u8>> {
    fn available(&self) -> io::Result<u64> { Ok(self.get_ref().len() as u64 - self.position()) }
}

impl SnappyRead for BufReader<File> {
    fn available(&self) -> io::Result<u64> {
        let metadata = try!(self.get_ref().metadata());
        Ok(metadata.len())
    }
}

/// Small, fixed-size, non-allocating queue of positions of prefixes in the Dict.
/// When a new element is added, the oldest is removed.
#[derive(Copy, Clone)]
struct PositionQueue {
    queue: [u16; MAX_CHAIN_LEN as usize],
    len: u8
}

impl PositionQueue {
    fn new() -> PositionQueue {
        PositionQueue {
            queue: [0; MAX_CHAIN_LEN as usize],
            len: 0
        }
    }

    fn iter<'a>(&'a self) -> Iter<'a, u16> {
        self.queue[..self.len()].iter()
    }

    fn push(&mut self, pos: u16) {
        if self.len > 0 && MAX_CHAIN_LEN > 1 {
            if MAX_CHAIN_LEN == 2 {
                self.queue[1] = self.queue[0];
            } else if MAX_CHAIN_LEN == 3 {
                self.queue[2] = self.queue[1];
                self.queue[1] = self.queue[0];
            } else if MAX_CHAIN_LEN == 4 {
                self.queue[3] = self.queue[2];
                self.queue[2] = self.queue[1];
                self.queue[1] = self.queue[0];
            } else if MAX_CHAIN_LEN == 5 {
                self.queue[4] = self.queue[3];
                self.queue[3] = self.queue[2];
                self.queue[2] = self.queue[1];
                self.queue[1] = self.queue[0];
            } else {
                let len = cmp::min(self.len + 1, MAX_CHAIN_LEN) as usize;
                unsafe {
                    ptr::copy(self.queue.as_ptr(), self.queue.as_mut_ptr().offset(1), len - 1)
                }
            }
        }
        self.queue[0] = pos;
        self.len = cmp::min(self.len + 1, MAX_CHAIN_LEN);
    }

    fn len(&self) -> usize { self.len as usize }
}

pub struct CompressorOptions {
    pub block_size: u16,
}

impl Default for CompressorOptions {
    fn default() -> CompressorOptions {
        CompressorOptions {
            block_size: MAX_BLOCK_SIZE as u16,
        }
    }
}

struct LossyHashTable {
    table: Vec<(u32, PositionQueue)>,
    /// Mask that performs the function of the usual module in hash tables
    range_mask: u32
}

impl LossyHashTable {
    fn new(capacity: u32) -> LossyHashTable {
        let real_capacity = cmp::max(16, next_power_of_2(capacity));
        LossyHashTable {
            table: vec![(0, PositionQueue::new()); real_capacity as usize],
            range_mask: real_capacity - 1
        }
    }

    fn get_or_insert<'a>(&'a mut self, key: &[u8], pos: u16) -> Option<&'a mut PositionQueue> {
        debug_assert_eq!(key.len(), MIN_COPY_LEN as usize);
        let key = unsafe { ptr::read(key.as_ptr() as *const u32) };
        let idx = self.hash(key);
        // We know idx is always in range, but using safe indexing is for some reason faster.
        let &mut (ref mut stored_key, ref mut queue) = &mut self.table[idx];
        if queue.len() != 0 && *stored_key == key {
            return Some(queue);
        } else {
            *stored_key = key;
            queue.len = 0;
            queue.push(pos);
            return None;
        }
    }


    fn clear(&mut self) {
        for e in self.table.iter_mut() {
            e.1.len = 0;
        }
    }

    // Used for hashing prefixes.
    // A good hash function means better compression, since there will be fewer collisions.
    fn hash(&self, key: u32) -> usize {
        let mut a = (key ^ 61) ^ (key >> 16);
        a = a.wrapping_add(a << 3);
        a = a ^ (a >> 4);
        a = a.wrapping_mul(0x27d4eb2d);
        a = a ^ (a >> 15);
        (a & self.range_mask) as usize
    }
}

struct Dict {
    table: LossyHashTable
}

impl Dict {
    fn new(capacity: u32) -> Dict {
        Dict {
             table: LossyHashTable::new(capacity)
        }
    }

    fn clear(&mut self) {
        self.table.clear();
    }

    /// Returns (position, length)
    fn find_best_match_or_add(&mut self, block: &[u8], start: usize) -> Option<(u16, u16)> {
        let prefix = &block[start..start + MIN_COPY_LEN as usize];
        let positions = match self.table.get_or_insert(prefix, start as u16) {
            None     => return None,
            Some(ps) => ps
        };

        let mut best_pos;
        let mut best_len;
        {
            let mut posit = positions.iter();
            best_pos = *posit.next().unwrap();
            // We already know the first MIN_COPY_LEN bytes are equal, no need to search through them.
            let search_start = start as u16 + MIN_COPY_LEN;
            best_len = MIN_COPY_LEN + common_prefix_length(block, best_pos + MIN_COPY_LEN, search_start);
            for &pos in posit {
                let len = MIN_COPY_LEN + common_prefix_length(block, pos + MIN_COPY_LEN, search_start);
                if len > best_len {
                    best_pos = pos;
                    best_len = len;
                }
            }
        }
        positions.push(start as u16);
        Some((best_pos, best_len))
    }
}


pub fn compress<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W) -> io::Result<()> {
    compress_with_options(inp, out, &Default::default())
}

// TODO Report an error on readers with too much available()
#[inline(never)]
pub fn compress_with_options<R: SnappyRead, W: Write>(inp: &mut R, out: &mut W,
                                                   options: &CompressorOptions) -> io::Result<()> {
    debug_assert!(inp.available().unwrap() <= ::std::u32::MAX as u64);
    let uncompressed_length = try!(inp.available()) as u32;
    try!(write_varint(out, uncompressed_length));
    let max_block_len = cmp::min(options.block_size as u32, uncompressed_length);
    let mut dict = Dict::new(max_block_len / 8);  // TODO Figure out capacity
    let mut written = 0;
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
                written += chunk.len() as u32;
                if written < uncompressed_length {
                    dict.clear();
                }
            }
        }
        inp.consume(len);
    }
}

fn compress_block<W: Write>(block: &[u8], out: &mut W, dict: &mut Dict)
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
                    copy_offset = i as u16 - pos;
                    copy_len = len;
                    break;
                }
            }
            i += 1;
            if i >= imax { break 'outer; }
        }

        try!(emit_literal(out, &block[literal_start..i]));

        loop {
            i += copy_len as usize;
            try!(emit_copy(out, copy_offset, copy_len));
            literal_start = i;
            if i >= imax { break 'outer; }
            match dict.find_best_match_or_add(block, i) {
                None => break,
                Some((pos, len)) => {
                    copy_offset = i as u16 - pos;
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

/// Emits a copy of any size, possibly emitting multiple copy tags.
fn emit_copy<W: Write>(out: &mut W, offset: u16, len: u16) -> io::Result<()> {
    debug_assert!(len >= MIN_COPY_LEN);
    //writeln!(io::stderr(), "<copy len={} offset={}>", len, offset);
    let mut remaining = len;
    while remaining >= MAX_COPY_LEN + MIN_COPY_LEN {
        try!(do_emit_copy(out, offset, MAX_COPY_LEN));
        remaining -= MAX_COPY_LEN;
    }
    if remaining > MAX_COPY_LEN {
        let to_emit = MAX_COPY_LEN - MIN_COPY_LEN;
        try!(do_emit_copy(out, offset, to_emit));
        remaining -= to_emit;
    }
    // We've made sure not to emit the last MIN_COPY_LEN, so we don't need a check here.
    do_emit_copy(out, offset, remaining)
}

fn do_emit_copy<W: Write>(out: &mut W, offset: u16, len: u16) -> io::Result<()> {
    debug_assert!(len >= MIN_COPY_LEN);
    debug_assert!(len <= MAX_COPY_LEN);
    let len = len as u8;
    if len <= 11 && offset <= 2047 {
        let n = len - 4;
        let tag = (n << 2) | COPY_1_BYTE | ((offset >> 3) & 0xE0) as u8;
        let low_len = (offset & 0xFF) as u8;
        try!(out.write(&[tag, low_len]));
    } else {
        let n = len - 1;
        let tag = (n << 2) | COPY_2_BYTE;
        try!(out.write(&[tag]));
        try!(write_u16_le(out, offset as u16));
    }
    Ok(())
}

fn emit_literal<W: Write>(out: &mut W, literal: &[u8]) -> io::Result<()> {
    debug_assert!(literal.len() < ::std::u32::MAX as usize);
    //writeln!(io::stderr(), "<literal len={}>", literal.len());
    let len = literal.len() - 1;
    if len < 60 {
        let tag = ((len as u8) << 2) | LITERAL;
        try!(out.write(&[tag]));
        try!(out.write_all(literal));
    } else {
        let mut ds = [0, 0, 0, 0];
        let mut n = native_to_le32(len as u32);
        let mut count = 0;
        while n > 0 {
            ds[count + 1] = (n & 0xFF) as u8;
            n >>= 8;
            count += 1;
        }
        let tag = (((59 + count) as u8) << 2) | LITERAL;
        ds[0] = tag;
        try!(out.write(&ds[..count + 1]));
        try!(out.write_all(literal));
    }
    Ok(())
}

/// Find the length of the common prefix of slices in block starting at a and b.
fn common_prefix_length(block: &[u8], a: u16, b: u16) -> u16 {
    let s1 = block[a as usize..].as_ptr();
    let s2 = block[b as usize..].as_ptr();
    unsafe {
        // TODO The cast to isize "can" (not really due to block size limitations) overflow
        let s2_limit = block.as_ptr().offset(block.len() as isize);
        find_match_length(s1, s2, s2_limit) as u16
    }
}

// Largely borrowed from the reference Snappy implementation
// Does not read *s2_limit or beyond.
// Does not read *(s1 + (s2_limit - s2)) or beyond.
// Requires that s2_limit >= s2.
#[cfg(all(target_pointer_width = "64", target_endian = "little"))]
unsafe fn find_match_length(s1: *const u8, mut s2: *const u8, s2_limit: *const u8) -> u32 {
    unsafe fn load64(p: *const u8) -> u64 { ptr::read(p as *const u64) }

    let mut matched = 0;
    while s2 <= s2_limit.offset(-8) {
        if load64(s2) == load64(s1.offset(matched as isize)) {
            s2 = s2.offset(8);
            matched += 8;
        } else {
            let x = load64(s2) ^ load64(s1.offset(matched as isize));
            let matching_bits = util::find_lsb_set64(x);
            matched += matching_bits / 8;
            return matched;
        }
    }
    while s2 < s2_limit {
        if *s1.offset(matched as isize) == *s2 {
            s2 = s2.offset(1);
            matched += 1;
        } else {
            return matched;
        }
    }
    return matched;
}

#[cfg(not(all(target_pointer_width = "64", target_endian = "little")))]
unsafe fn find_match_length(s1: *const u8, mut s2: *const u8, s2_limit: *const u8) -> u32 {
    unsafe fn load_32(p: *const u8) -> u32 { ptr::read(p as *const u32) }

    let mut matched = 0;
    while s2 <= s2_limit.offset(-4) && load32(s2) == load32(s1.offset(matched as isize)) {
        s2 = s2.offset(4);
        matched += 4;
    }
    if cfg!(target_endian = "little") {
        let x = load32(s2) ^ load32(s1.offset(matched as isize));
        let matching_bits = util::find_lsb_set32(x);
        matched += matching_bits / 8;
    } else {
        while s2 < s2_limit && *s1.offset(matched as isize) == *s2 {
            s2 = s2.offset(1);
            matched += 1;
        }
    }
    return matched;
}

fn write_u16_le<W: Write>(out: &mut W, n: u16) -> io::Result<()> {
    let le = native_to_le16(n);
    let ptr = &le as *const u16 as *const u8;
    let s = unsafe { ::std::slice::from_raw_parts(ptr, 2) };
    try!(out.write(s));
    Ok(())
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
    use super::{write_varint, emit_literal, emit_copy, common_prefix_length};

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

    #[test]
    fn test_emit_copy_large() {
        let mut out = Vec::new();
        emit_copy(&mut out, 60_000, 40).unwrap();
        assert_eq!(&out[..], &[0b100111_10, 0x60, 0xEA]);
    }

    #[test]
    fn test_common_prefix_length() {
        assert_eq!(common_prefix_length(&[1, 2, 3, 4, 5, 3, 4, 5], 2, 5), 3);
        assert_eq!(common_prefix_length(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
                                                3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 2, 11), 9);
        assert_eq!(common_prefix_length(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
                                                3, 4, 5, 6, 0, 8, 9, 10, 11, 12], 2, 11), 4);
    }
}