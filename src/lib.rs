mod decompress;
mod compress;
mod util;

pub use compress::{compress, compress_with_options, CompressorOptions, SnappyRead};
pub use decompress::{decompress, SnappyWrite};