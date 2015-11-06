extern crate rsnappy;

use std::io::Cursor;
use std::cmp;
use rsnappy::{compress, decompress};

static TEXT: &'static str = include_str!("moonstone-short.txt");

macro_rules! decompress(
    ($input: expr, $output: expr) => (
        match decompress(&mut Cursor::new(&$input[..]), $output) {
            Err(e) => {
                println!("failed: {:?}", e);
                println!("remaining input: {:?}", &$input[..]);
                panic!("failed: {:?}", e)
            },
            Ok(_)  => {}
        };
    )
);

macro_rules! compress(
    ($input: expr, $output: expr) => (
        match compress(&mut Cursor::new(&$input[..]), $output) {
            Err(e) => {
                println!("failed: {:?}", e);
                println!("remaining input: {:?}", &$input[..]);
                println!("written ouput: {:?}", &$output[..]);
                panic!("failed: {:?}", e)
            },
            Ok(_)  => {}
        };
    )
);

#[test]
fn test_big_roundtrip() {
    test_roundtrip(TEXT.as_bytes());
}

#[test]
fn test_small_roundtrip() {
    let inp: Vec<u8> = (0..80).collect();
    test_roundtrip(&inp[..]);
}

#[test]
fn test_tiny_roundtrip() {
    test_roundtrip(&[1]);
}

#[test]
fn test_empty_roundtrip() {
    test_roundtrip(&[]);
}

fn test_roundtrip(inp: &[u8]) {
    let mut out = Vec::new();
    compress!(inp, &mut out);
    println!("compressed {} => {}", inp.len(), out.len());
    let mut roundtrip = Vec::new();
    decompress!(&out[..], &mut roundtrip);
    let n = cmp::min(inp.len(), 384);
    assert_eq!(&inp[..n], &roundtrip[..n]);  // Hopefully avoid some very long error messages
    assert_eq!(inp, &roundtrip[..]);
}
