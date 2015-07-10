use std::env;
use std::fs::{File, read_dir};
use std::io::Write;
use std::path::Path;
use std::iter::IntoIterator;

fn main() {
    let s = env::var("OUT_DIR").unwrap();
    let out_dir = Path::new(&s);
    write_tables_rs(&out_dir);
    write_benchmarks(&out_dir);
}

fn write_tables_rs(out_dir: &Path) {
    let dest_path = out_dir.join("tables.rs");
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
    f.write_all(b"];\n").unwrap();

    f.write_all(b"#[inline]\nfn get_tag_size(c: u8) -> usize { (TAG_SIZE[c as usize] + 1) as usize }\n").unwrap();
}

fn write_benchmarks(out_dir: &Path) {
    let s = env::var("CARGO_MANIFEST_DIR").unwrap();
    let bench_data_dir = Path::new(&s).join("benches").join("data");
    let bench_files = read_dir(bench_data_dir).unwrap().map(|e| {
        let e = e.unwrap();
        let mut s = String::new();
        for &c in e.path().file_name().unwrap().to_string_lossy().as_bytes() {
            if (c >= b'0' && c <= b'9') || (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') || c == b'_' {
                s.push(c as char);
            } else {
                s.push('_');
            }
        }
        (e.path().to_string_lossy().to_string(), s)
    });
    do_write_benchmarks(out_dir, bench_files);
}

fn do_write_benchmarks<I: IntoIterator<Item=(String, String)>>(out_dir: &Path, files: I) {
    let mut f = File::create(out_dir.join("generated-benches.rs")).unwrap();
    writeln!(f, "extern crate test;").unwrap();
    writeln!(f, "extern crate snappy;").unwrap();
    writeln!(f, "use std::io::{{Read, Cursor}};").unwrap();
    writeln!(f, "use std::path::Path;").unwrap();
    writeln!(f, "use std::fs::File;").unwrap();
    for (path, name) in files {
        writeln!(f,r##"
#[bench]
fn bench_{}(bench: &mut test::Bencher) {{
    let mut f = File::open(Path::new("{}")).unwrap();
    let mut input = Vec::new();
    f.read_to_end(&mut input);
    let mut output = Vec::with_capacity(input.len() * 2);
    bench.iter(|| {{
        snappy::compress(&mut Cursor::new(&input[..]), &mut output).unwrap();
        output.clear();
    }});
    bench.bytes = input.len() as u64;
}}"##, name, path).unwrap();
    }
}
