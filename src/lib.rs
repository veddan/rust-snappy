mod decompress;
mod compress;
mod util;
mod zero_array;

pub use compress::{compress, compress_with_options, CompressorOptions, SnappyRead, MAX_BLOCK_SIZE};
pub use decompress::{decompress, SnappyWrite};