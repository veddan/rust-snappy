extern crate rsnappy;

use std::io::Cursor;
use rsnappy::{decompress};

macro_rules! decompress(
    ($input: expr, $output: expr) => (
        match decompress(&mut Cursor::new(&$input[..]), $output) {
            Err(e) => {
                println!("failed: {:?}", e);
                let (ellipsis, len) = if $input.len() > 16 {
                    ("...", 16)
                } else {
                    ("", $input.len())
                };
                println!("remaining input: {:?}{}", &$input[..len], ellipsis);
                panic!("failed: {:?}", e)
            },
            Ok(_)  => {}
        };
    )
);

#[test]
fn test_small_literal() {
    let input = vec![7u8 /* uncompressed length */, 6 << 2 /* 7-byte literal */, 1, 2, 3, 4, 5, 6, 7];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn test_two_small_literals() {
    let input = vec![7u8 /* uncompressed length */,
        2 << 2 /* 3-byte literal */, 1, 2, 3,
        3 << 2 /* 4-byte literal */, 4, 5, 6, 7];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn test_big_literal() {
    let mut expected_out = vec![];
    for i in 0..1_000_000 { expected_out.push((i % 10) as u8); }
    let mut input = vec![64 | 0x80, 4 | 0x80, 61, /* uncompressed length */
                        63 << 2 /* long 4-byte literal */,
                        0x3F, 0x42, 0x0F, 0x00 /* 1_000_000_000 in little endian */];
    for &i in expected_out.iter() { input.push(i); }
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &expected_out[..]);
}

#[test]
fn test_1byte_copy() {
    let input = vec![11u8 /* uncompressed length */,
        5 << 2 /* 6-byte literal */, 1, 2, 3, 4, 5, 6,
        0b00000101 /* 1-byte offset copy, 5 bytes */, 6 /* 6-byte offset */];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5]);
}

#[test]
fn test_2byte_copy() {
    let input = vec![9u8 /* uncompressed length */,
        5 << 2 /* 6-byte literal */, 1, 2, 3, 4, 5, 6,
        0b00001010 /* 2-byte offset copy, 3 bytes */, 5, 0 /* 5-byte offset */];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 4, 5, 6, 2, 3, 4]);
}

#[test]
fn test_4byte_copy() {
    let input = vec![9u8 /* uncompressed length */,
        5 << 2 /* 6-byte literal */, 1, 2, 3, 4, 5, 6,
        0b00001011 /* 5-byte offset copy, 3 bytes */, 5, 0, 0, 0 /* 5-byte offset */];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 4, 5, 6, 2, 3, 4]);
}

#[test]
fn test_repeat_copy() {
        let input = vec![7u8 /* uncompressed length */,
        2 << 2 /* 3-byte literal */, 1, 2, 3,
        0b00000001 /* 1-byte offset copy, 4 bytes */, 2 /* 4-byte offset */];
    let mut output = vec![];
    decompress!(&input, &mut output);
    assert_eq!(&output[..], &[1, 2, 3, 2, 3, 2, 3]);
}

#[test]
fn test_decompress_malformed() {
    let n = 114045;
    let mut input = vec![250, 134, 252, 255, 255, 0, 0, 84, 104, 101, 32];
    input.reserve(n);
    while input.len() < n {
        input.push(0);
    }
    let mut out = Vec::new();
    let _ = decompress(&mut Cursor::new(&input[..]), &mut out);
}
