#![feature(test)]

extern crate test;
extern crate rand;
extern crate snappy;

use std::io::Cursor;
use rand::{weak_rng, Rng};
use snappy::{compress, decompress};

static TEXT: &'static str = include_str!("../tests/moonstone-short.txt");

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

#[bench]
fn bench_compress_text(bench: &mut test::Bencher) {
    bench.iter(|| {
        let mut out = Vec::with_capacity(TEXT.len());
        compress!(TEXT.as_bytes(), &mut out);
    });
    bench.bytes = TEXT.len() as u64;
}

#[bench]
fn bench_compress_short_text(bench: &mut test::Bencher) {
    let input = &TEXT.as_bytes()[727..2000];
    bench.iter(|| {
        let mut out = Vec::with_capacity(input.len());
        compress!(input, &mut out);
    });
    bench.bytes = input.len() as u64;
}

#[bench]
fn bench_compress_random(bench: &mut test::Bencher) {
    let mut rng = weak_rng();
    let input: Vec<u8> = (0..TEXT.len()).map(|_| rng.gen()).collect();
    bench.iter(|| {
        let mut out = Vec::with_capacity(input.len());
        compress!(&input[..], &mut out);
    });
    bench.bytes = input.len() as u64;
}

#[bench]
fn bench_decompress_text(bench: &mut test::Bencher) {
    let mut compressed = Vec::new();
    compress!(TEXT.as_bytes(), &mut compressed);
    bench.iter(|| {
        let mut out = Vec::with_capacity(TEXT.len());
        decompress!(compressed[..], &mut out);
    });
    bench.bytes = TEXT.len() as u64;
}