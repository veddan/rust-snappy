extern crate snappy;
extern crate rustc_serialize;
extern crate docopt;

use std::io;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::fs::File;
use std::default::Default;
use std::process::exit;
use snappy::{decompress, compress_with_options, CompressorOptions, MAX_BLOCK_SIZE};
use docopt::Docopt;

static USAGE: &'static str = "
Usage: rsnap [options] <src>
       rsnap --help

Options:
  -h, --help             Show this message.
  -d, --decompress       Decompress.
  -b, --block-size=<kb>  Sets compressor block size.
                         There is no simple relationship between block size and
                         performance or compressed size.
";

#[derive(RustcDecodable, Debug)]
struct Args {
    arg_src: String,
    flag_decompress: bool,
    flag_block_size: Option<usize>
}

fn main() {
    let args: Args = Docopt::new(USAGE).and_then(|d| d.decode()).unwrap_or_else(|e| e.exit());
    let path = Path::new(&args.arg_src);
    let file = File::open(path).unwrap();
    let mut input = BufReader::new(file);
    if args.flag_decompress {
        let mut output = Vec::new();
        decompress(&mut input, &mut output).unwrap();
        io::stdout().write_all(&output[..]).unwrap();
    } else {
        let mut output = BufWriter::new(io::stdout());
        let mut options = CompressorOptions::default();
        args.flag_block_size.map(|m|{
            let bytes = match m.checked_mul(1024).and_then(|x| {
                    if x > ::std::u16::MAX as usize { None } else { Some(x as u16) } }) {
                Some(b) => b,
                None    => {
                    writeln!(io::stderr(), "Chosen block size {}kb is greater than the maximum {}kb",
                             m, MAX_BLOCK_SIZE / 1024).unwrap();
                    exit(1);
                }
            };
            options.block_size = bytes;
        });
        compress_with_options(&mut input, &mut output, &options).unwrap();
    }
}