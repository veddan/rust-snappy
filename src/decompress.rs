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
    FormatError,
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
                //println!("[tag] read from reader: {:?} ({} bytes)",
                            //&b[..cmp::min(11, b.len())], b.len());
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
            //println!("available: {}", self.available());
            let (buf, buf_end) = if self.available() == 0 {
                self.reader.consume(self.read);
                self.read = 0;
                read_new_buffer!(self)
            } else {
                (self.buf, self.buf_end)
            };
            let c = ptr::read(buf);
            let tag_size = get_tag_size(c) + 1;
            let mut buf_len = buf_end as usize - buf as usize;
            if buf_len < tag_size {
                ptr::copy_nonoverlapping(buf, self.tmp.as_mut_ptr(), buf_len);
                self.reader.consume(self.read);
                self.read = 0;
                while buf_len < tag_size {
                    let (newbuf, newbuf_end) = read_new_buffer!(self, return Err(FormatError));
                    let newbuf_len = newbuf_end as usize - newbuf as usize;
                    let to_read = cmp::min(tag_size - buf_len, newbuf_len);  // How many bytes should we read from the new buffer?
                    ptr::copy_nonoverlapping(newbuf, self.tmp.as_mut_ptr(), to_read);
                    buf_len += to_read;
                    self.reader.consume(to_read);
                }
                self.buf = self.tmp.as_ptr();
                self.buf_end = self.buf.offset(buf_len as isize);
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
            let _tag_size = try_advance_tag!(self);
            //println!("read {} byte tag", _tag_size);
            let c = self.read(1)[0];
            if c & 0x03 == 0 {  // literal
                let lenbits = ((c & !0x03) >> 2) + 1;
                let literal_len = if lenbits <= 60 {  // TODO use tag_size
                    lenbits as u32
                } else {
                    let literal_len_bytes = lenbits - 60;
                    let n = self.read_u32_le(literal_len_bytes) + 1;
                    n
                };
                //println!("<literal len={}>", literal_len);
                let mut remaining = literal_len as usize;
                while self.available() < remaining {
                    let available = self.available();
                    match writer.write_all(self.read(available)) {
                        Ok(_)  => { },
                        Err(e) => return Err(IoError(e))
                    };
                    remaining -= available;
                    //println!("wrote {}, {} remaining", available, remaining);
                    self.reader.consume(self.read);
                    match self.reader.fill_buf() {
                        Err(e) => return Err(IoError(e)),
                        Ok(b) if b.len() == 0 => {
                            return Err(FormatError);
                        },
                        Ok(b) => {
                            //println!("[literal] read from reader: {:?}", b);
                            self.buf = b.as_ptr();
                            self.buf_end = unsafe {  b.as_ptr().offset(b.len() as isize) };
                            self.read = b.len();
                        }
                    }
                }
                match writer.write_all(self.read(remaining)) {
                    Ok(_)  => { },
                    Err(e) => return Err(IoError(e))
                };
            } else {  // copy
                let (copy_len, copy_offset) = if _tag_size == 2 {
                    let len = 4 + ((c & 0x1C) >> 2);
                    let offset = (((c & 0xE0) as u32) << 3) | self.read(1)[0] as u32;
                    (len, offset)
                } else if _tag_size == 3 {
                    let len = 1 + (c >> 2);
                    let offset = self.read_u16_le() as u32;
                    (len, offset)
                } else {
                    let len = 1 + (c >> 2);
                    let offset = self.read_u32_le(4);
                    (len, offset)
                };
                //println!("<copy len={} offset={}>", copy_len, copy_offset);
                if copy_offset == 0 {  // zero-length copies can't be encoded, no need to check
                    return Err(FormatError);
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
        self.read += n as usize;
        self.buf = unsafe { self.buf.offset(n as isize) };
        return r;
    }

    fn available(&self) -> usize {
        self.buf_end as usize - self.buf as usize
    }

    fn get_buf(&self) -> &[u8] {
        unsafe { ::std::slice::from_raw_parts(self.buf, self.available()) }
    }

    fn read_u32_le(&mut self, bytes: u8) -> u32 {
        const MASKS: &'static [u32] = &[0, 0xFF000000, 0xFFFF0000, 0xFFFFFF00, 0xFFFFFFFF];
        let p = self.read(4).as_ptr() as *const u32;
        let x = unsafe { ptr::read(p) } & MASKS[bytes as usize];
        return native_to_le32(x);
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
        Ok(buf) if buf.len() == 0 => return Err(FormatError),
        Ok(buf) => {
            for c in buf.iter() {
                if shift >= 32 { return Err(FormatError); }
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
        //println!("uncompressed length: {}", result);
        Ok(result)
    } else {
        Err(FormatError)
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
