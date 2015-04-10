extern crate snappy;
extern crate rustc_serialize;
extern crate docopt;

use std::io;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::fs::File;
use std::default::Default;
use snappy::{decompress, compress_with_options, CompressorOptions};
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
    flag_block_size: Option<u32>
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
        args.flag_block_size.map(|m| options.block_size = m * 1024);
        compress_with_options(&mut input, &mut output, &options).unwrap();
    }
}