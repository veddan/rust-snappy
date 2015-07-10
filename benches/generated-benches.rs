#![feature(test)]
#[allow(unused_must_use)]
#[allow(non_snake_case)]
mod generated_benches {
include!(concat!(env!("OUT_DIR"), "/generated-benches.rs"));
}

