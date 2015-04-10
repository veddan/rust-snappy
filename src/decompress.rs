use std::io::{Write, BufRead};
use std::io;
use std::ptr;
use std::cmp;
use std::result::Result;
use util::{native_to_le16, native_to_le32};
use self::SnappyError::*;

include!(concat!(env!("OUT_DIR"), "/tables.rs"));

const MAX_TAG_LEN: usize = 5;

pub trait SnappyWrite : Write {
    fn write_from_self(&mut self, offset: usize, len: usize) -> io::Result<()>;
}

#[derive(Debug)]
pub enum SnappyError {
    FormatError(&'static str),
    IoError(io::Error)
}

struct Decompressor<R> {
    reader: R,
    tmp: [u8; MAX_TAG_LEN],
    buf: *const u8,
    buf_end: *const u8,
    read: usize,
}

macro_rules! try_advance_tag {
    ($me: expr) => (
        match $me.advance_tag() {
            Ok(None)            => return Ok(()),
            Ok(Some(tag_size))  => tag_size,
            Err(e)              => return Err(e)
        }
    )
}

macro_rules! read_new_buffer {
    ($me: expr) => (
        read_new_buffer!($me, return Ok(None))
    );
    ($me: expr, $on_eof: expr) => (
        match $me.reader.fill_buf() {
            Err(e) => return Err(IoError(e)),
            Ok(b) if b.len() == 0 => {
                $on_eof
            },
            Ok(b) => {
                (b.as_ptr(), b.as_ptr().offset(b.len() as isize))
            }
        }
    );
}

impl <R: BufRead> Decompressor<R> {
    fn new(reader: R) -> Decompressor<R> {
        Decompressor {
            reader: reader,
            tmp: [0; MAX_TAG_LEN],
            buf: ptr::null(),
            buf_end: ptr::null(),
            read: 0,
        }
    }

    fn advance_tag(&mut self) -> Result<Option<usize>, SnappyError> {
        unsafe {
            let buf;
            let buf_end;
            let mut buf_len;
            if self.available() == 0 {
                self.reader.consume(self.read);
                let (b, be) = read_new_buffer!(self);
                buf = b;
                buf_end = be;
                buf_len = buf_end as usize - buf as usize;
                self.read = buf_len;
            } else {
                buf = self.buf;
                buf_end = self.buf_end;
                buf_len = self.available();
            };
            let c = ptr::read(buf);
            let tag_size = get_tag_size(c);
            if buf_len < tag_size {
                ptr::copy_nonoverlapping(buf, self.tmp.as_mut_ptr(), buf_len);
                self.reader.consume(self.read);
                self.read = 0;
                while buf_len < tag_size {
                    let (newbuf, newbuf_end) = read_new_buffer!(self,
                            return Err(FormatError("EOF while reading tag")));
                    let newbuf_len = newbuf_end as usize - newbuf as usize;
                    let to_read = cmp::min(tag_size - buf_len, newbuf_len);  // How many bytes should we read from the new buffer?
                    ptr::copy(newbuf, self.tmp.as_mut_ptr().offset(buf_len as isize), to_read);
                    buf_len += to_read;
                    self.reader.consume(to_read);
                }
                self.buf = self.tmp.as_ptr();
                self.buf_end = self.buf.offset(tag_size as isize);
            } else if buf_len < MAX_TAG_LEN {
                ptr::copy_nonoverlapping(buf, self.tmp.as_mut_ptr(), buf_len);
                self.reader.consume(self.read);
                self.read = 0;
                self.buf = self.tmp.as_ptr();
                self.buf_end = self.buf.offset(buf_len as isize);
            } else {
                self.buf = buf;
                self.buf_end = buf_end;
            }
            Ok(Some(tag_size))
        }
    }

    fn decompress<W: SnappyWrite>(&mut self, writer: &mut W) -> Result<(), SnappyError> {
        loop {
            let tag_size = try_advance_tag!(self);
            let c = self.read(1)[0];
            if c & 0x03 == 0 {  // literal
                let literal_len = if tag_size == 1 {
                    ((c >> 2) as u32) + 1
                } else {
                    let literal_len_bytes = (tag_size - 1) as u8;
                    self.read_u32_le(literal_len_bytes) + 1
                };
                let mut remaining = literal_len as usize;
                while self.available() < remaining {
                    let available = self.available();
                    match writer.write_all(self.read(available)) {
                        Ok(_)  => { },
                        Err(e) => return Err(IoError(e))
                    };
                    remaining -= available;
                    self.reader.consume(self.read);
                    match self.reader.fill_buf() {
                        Err(e) => return Err(IoError(e)),
                        Ok(b) if b.len() == 0 => {
                            return Err(FormatError("EOF while reading literal"));
                        },
                        Ok(b) => {
                            self.buf = b.as_ptr();
                            self.buf_end = unsafe { b.as_ptr().offset(b.len() as isize) };
                            self.read = b.len();
                        }
                    }
                }
                match writer.write_all(self.read(remaining)) {
                    Ok(_)  => { },
                    Err(e) => return Err(IoError(e))
                };
            } else {  // copy
                let (copy_len, copy_offset) = if tag_size == 2 {
                    let len = 4 + ((c & 0x1C) >> 2);
                    let offset = (((c & 0xE0) as u32) << 3) | self.read(1)[0] as u32;
                    (len, offset)
                } else if tag_size == 3 {
                    let len = 1 + (c >> 2);
                    let offset = self.read_u16_le() as u32;
                    (len, offset)
                } else {
                    let len = 1 + (c >> 2);
                    let offset = self.read_u32_le(4);
                    (len, offset)
                };
                if copy_offset == 0 {  // zero-length copies can't be encoded, no need to check for them
                    return Err(FormatError("zero-length offset"));
                }
                match writer.write_from_self(copy_offset as usize, copy_len as usize) {
                    Ok(_)  => {},
                    Err(e) => return Err(IoError(e))
                }
            }
        }
    }

    fn read(&mut self, n: usize) -> &[u8] {
        assert!(n as usize <= self.available());
        let r = unsafe { ::std::slice::from_raw_parts(self.buf, n) };
        self.advance(n);
        return r;
    }

    fn advance(&mut self, n: usize) {
        assert!(self.available() >= n);
        self.buf = unsafe { self.buf.offset(n as isize) };
    }

    fn available(&self) -> usize {
        self.buf_end as usize - self.buf as usize
    }

    fn get_buf(&self) -> &[u8] {
        unsafe { ::std::slice::from_raw_parts(self.buf, self.available()) }
    }

    fn read_u32_le(&mut self, bytes: u8) -> u32 {
        const MASKS: &'static [u32] = &[0, 0x000000FF, 0x0000FFFF, 0x00FFFFFF, 0xFFFFFFFF];
        let p = self.buf as *const u32;
        self.advance(bytes as usize);
        native_to_le32(unsafe { ptr::read(p) }) & MASKS[bytes as usize]
    }

    fn read_u16_le(&mut self) -> u16 {
        let p = self.read(2).as_ptr() as *const u16;
        let x = unsafe { ptr::read(p) };
        return native_to_le16(x);
    }
}

#[inline(never)]
pub fn decompress<R: BufRead, W: SnappyWrite>(reader: &mut R, writer: &mut W) -> Result<(), SnappyError> {
    let _uncompressed_length = try!(read_uncompressed_length(reader));
    let mut decompressor = Decompressor::new(reader);
    decompressor.decompress(writer)
}

fn read_uncompressed_length<R: BufRead>(reader: &mut R) -> Result<u32, SnappyError> {
    let mut result: u32 = 0;
    let mut shift = 0;
    let mut success = false;
    let mut read = 1;
    // This is a bit convoluted due to working around a borrowing issue with buf and reader.consume().
    match reader.fill_buf() {
        Err(e) => return Err(IoError(e)),
        Ok(buf) if buf.len() == 0 => return Err(FormatError("premature EOF")),
        Ok(buf) => {
            for c in buf.iter() {
                if shift >= 32 { return Err(FormatError("uncompressed length exceeds u32::MAX")); }
                result |= ((c & 0x7F) as u32) << shift;
                if (c & 0x80) == 0 {
                    success = true;
                    break;
                }
                shift += 7;
                read += 1;
            }
        }
    }
    if success {
        reader.consume(read);
        Ok(result)
    } else {
        Err(FormatError("unterminated uncompressed length"))
    }
}

impl SnappyWrite for Vec<u8> {
    fn write_from_self(&mut self, offset: usize, len: usize) -> io::Result<()> {
        let start = self.len() - offset;  // FIXME overflow on [250, 134, 252, 255, 255, 0, 0, 84, 104, 101, 32, ..] (114045 bytes total)
        for i in (0..len) {
            let c = self[start + i];
            self.push(c);
        }
        return Ok(());
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;
    use super::read_uncompressed_length;

    #[test]
    fn test_read_uncompressed_length_long() {
        let inp = [0xFE, 0xFF, 0x7F];
        assert_eq!(read_uncompressed_length(&mut Cursor::new(&inp[..])).unwrap(), 2097150);
    }

    #[test]
    fn test_read_uncompressed_length_short() {
        let inp = [64];
        assert_eq!(read_uncompressed_length(&mut Cursor::new(&inp[..])).unwrap(), 64);
    }
}
