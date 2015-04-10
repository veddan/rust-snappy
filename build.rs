use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;


fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("tables.rs");
    let mut f = File::create(&dest_path).unwrap();

    f.write_all(b"const TAG_SIZE: [u32; 256] = [\n").unwrap();
    let mut c = 0;
    loop {
        let n = match c & 0x03 {
            0b00 => {  // literal
                let n = c >> 2;
                if n < 60 {
                    0
                } else {
                    n - 59
                }
            },
            0b01 => 1,  // copy with 1-byte offset
            0b10 => 2,  // copy with 2-byte offset
            0b11 => 4,  // copy with 4-byte offset
            _    => unreachable!()
        };
        write!(&mut f, "\t{},\n", n).unwrap();
        if c == 255 { break; }
        c += 1;
    }
    f.write_all(b"];\n\n").unwrap();

    f.write_all(b"fn get_tag_size(c: u8) -> usize { (TAG_SIZE[c as usize] + 1) as usize }\n").unwrap();
}