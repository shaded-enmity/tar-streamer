#[macro_use]
extern crate bitflags;
extern crate getopts;
extern crate flate2;
extern crate tar;
extern crate xz2;
extern crate zip;
extern crate bzip2;

use std::env;
use std::fs::File;
use std::path::Path;
use std::io::{Read, Write, Error};
use std::ascii::AsciiExt;
use std::process::Command;
use std::os::unix::fs::MetadataExt;
use getopts::Options;
use zip::ZipArchive;
use bzip2::read::BzDecoder;
use xz2::read::XzDecoder;
use flate2::read::GzDecoder;
use tar::{Builder, Header};

bitflags! {
    flags ArchiveType: u32 {
        const INVALID = 0b00000000,
        const TAR     = 0b00000001,
        const GZIP    = 0b00000010,
        const ZIP     = 0b00000100,
        const XZ      = 0b00001000, 
        const BZIP2   = 0b00010000,
        const _ALL    = (0b00010000 << 1) - 1
    }
}

// Chunked trait for reading in chunks of size of the buffer
trait Chunked {
    fn chunked<F>(&mut self, mut buffer: &mut [u8], mut callback: F) -> Result<usize, Error>
        where F: FnMut(&[u8], usize);
}

// Implement the Chunked trait for the Read trait
impl<R: Read> Chunked for R {
    fn chunked<F>(&mut self, mut buffer: &mut [u8], mut callback: F) -> Result<usize, Error>
        where F: FnMut(&[u8], usize)
    {
        let mut read_total = 0usize;

        loop {
            let read = try!(self.read(&mut buffer));
            read_total += read;

            if read > 0 {
                callback(&buffer, read);
            } else {
                break;
            }
        }

        Ok(read_total)
    }
}

struct ArchiveClass<'a> {
    class: ArchiveType,
    type_name: &'a str,
    file_fingerprint: &'a str,
}

#[allow(non_upper_case_globals)]
static Archives: [ArchiveClass<'static>; 5] = [ArchiveClass {
                                                   class: TAR,
                                                   type_name: "tar",
                                                   file_fingerprint: "tar archive",
                                               },
                                               ArchiveClass {
                                                   class: GZIP,
                                                   type_name: "gzip",
                                                   file_fingerprint: "gzip compressed data",
                                               },
                                               ArchiveClass {
                                                   class: ZIP,
                                                   type_name: "zip",
                                                   file_fingerprint: "Zip archive data",
                                               },
                                               ArchiveClass {
                                                   class: XZ,
                                                   type_name: "xz",
                                                   file_fingerprint: "XZ compressed data",
                                               },
                                               ArchiveClass {
                                                   class: BZIP2,
                                                   type_name: "bzip2",
                                                   file_fingerprint: "bzip2 compressed data",
                                               }];

static VERSION: &'static str = "0.1.0";

// Less verbose version of the panic!() macro
fn error(message: &str) {
    println!("{}", message);
    std::process::exit(1);
}

// Print out usage information and exit with specified exit code
fn usage(code: i32, program: &str, opts: &Options) {
    let banner = format!("Usage: {} [options] SRC DST", program);
    println!("{} - {}", program, VERSION);
    print!("{}", opts.usage(&banner));
    println!("\nMultiple parameters for the -t / --type argument can be specified\nby \
              separating elements with commas:\n\n    {} --type=gzip,tar some.tar.gz \
              other.tar",
             program);
    std::process::exit(code);
}

// If haystack contains needle then set a bitflag in flags
fn find_and_set_flag(haystack: &str, needle: &str, flags: &mut ArchiveType, set: ArchiveType) {
    if let Some(_) = haystack.find(needle) {
        *flags |= set;
    }
}

// Get type of the archive by using the file(1) tool and filename heuristics
fn get_archive_type(path: &str) -> Option<ArchiveType> {
    match Command::new("file")
              .arg(path)
              .output() {
        Ok(output) => {
            let file_output = String::from_utf8_lossy(&output.stdout);
            let mut typ = INVALID;

            // Match type identification from the file(1) tool
            for class in Archives.iter() {
                find_and_set_flag(&file_output, class.file_fingerprint, &mut typ, class.class);
            }

            // If there's '.tar' in the file name or the file extension
            // is .tgz classify the file as Tar
            find_and_set_flag(&path, ".tar", &mut typ, TAR);
            find_and_set_flag(&path, ".tgz", &mut typ, TAR);

            Some(typ)
        }
        Err(_) => None,
    }
}

// Parse -t / --type parameter from command line
fn opts_archive_type(typ: &str, verbose: bool) -> ArchiveType {
    // First split the input string by comma, then map each element
    // against the mapping table, yielding an ArchiveType flag, and finally
    // fold the whole sequence with binary OR
    let parsed = typ.split(",")
                    .map(|v| {
                        match Archives.iter()
                                      .position(|ref p| p.type_name == v.to_ascii_lowercase()) {
                            Some(index) => Archives[index].class,
                            None => {
                                if verbose {
                                    println!("Invalid --type flag: {}", v);
                                }

                                INVALID
                            }
                        }
                    })
                    .fold(INVALID, |acc, x| acc | x);

    parsed
}

// Write decompressed data from decoder into destination file by using the provided buffer
fn decode_file_into<T: Chunked>(mut buffer: &mut [u8], dst: &mut File, mut decoder: T) {
    decoder.chunked(&mut buffer, |buf, read| {
               if dst.write(&buf[..read]).unwrap_or(0) != read {
                   error("Unable to write decompressed block");
               }
           })
           .unwrap();
}

// Stream source file into destination file
fn stream_file_into(src: &str,
                    dst: &str,
                    archive_type: ArchiveType,
                    block_size: usize,
                    verbose: bool) {
    let typ = match archive_type {
        INVALID => {
            match get_archive_type(src) {
                Some(t) => t,
                None => INVALID,
            }
        }
        _ => archive_type,
    };
    let mut target = File::create(dst).unwrap();
    let mut buffer: Vec<u8> = vec!(0u8; block_size);
    let file = File::open(src).unwrap();

    if typ.contains(GZIP) {
        if verbose {
            println!("GZip file");
        }

        let decoder = GzDecoder::new(file).unwrap();
        decode_file_into(&mut buffer, &mut target, decoder);
    } else if typ.contains(BZIP2) {
        if verbose {
            println!("BZip2 file");
        }

        let decoder = BzDecoder::new(file);
        decode_file_into(&mut buffer, &mut target, decoder);
    } else if typ.contains(XZ) {
        if verbose {
            println!("XZ file");
        }

        let decoder = XzDecoder::new(file);
        decode_file_into(&mut buffer, &mut target, decoder);
    } else if typ.contains(ZIP) {
        if verbose {
            println!("Zip file");
        }

        let file_meta = file.metadata().unwrap();
        let mut decoder = ZipArchive::new(&file).unwrap();
        let mut tar_builder = Builder::new(target);

        for i in 0..decoder.len() {
            // Get hold of ZipFile at particular index
            let zf = decoder.by_index(i).unwrap();

            // Create a Tar header for each ZipFile
            let mut tar_header = Header::new_gnu();

            // Set file metadata in tar header
            tar_header.set_size(zf.size());
            tar_header.set_path(Path::new(zf.name())).unwrap();
            tar_header.set_mode(zf.unix_mode().unwrap());
            tar_header.set_mtime(file_meta.mtime() as u64);
            tar_header.set_uid(file_meta.uid());
            tar_header.set_gid(file_meta.gid());
            tar_header.set_cksum();

            tar_builder.append(&tar_header, zf).unwrap();
        }

        tar_builder.finish().unwrap();
    } else if typ.contains(TAR) {
        if verbose {
            println!("Tar file");
        }

        decode_file_into(&mut buffer, &mut target, &file);
    } else {
        error(&format!("Unknown file type '{:?}' for '{}'", typ, src));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].split('/').last().unwrap();

    let mut opts = Options::new();
    opts.optflag("h", "help", "prints this menu");
    opts.optflag("v", "verbose", "verbose mode");
    opts.optflag("f", "force", "overwrite existing files");
    opts.optopt("t",
                "type",
                "input archive type(s)",
                "[GZIP, ZIP, BZIP2, XZ, TAR]");
    opts.optopt("b", "block-size", "size of processing block in bytes", "");
    opts.optflag("", "version", "display version information");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => panic!(e.to_string()),
    };

    if matches.opt_present("h") {
        // usage() terminates the program
        usage(0, &program, &opts);
    }

    if matches.free.len() != 2 {
        usage(1, &program, &opts);
    } else {
        let src = &matches.free[0];
        let dst = &matches.free[1];

        let src_path = Path::new(src);
        let dst_path = Path::new(dst);

        if !src_path.exists() || !src_path.is_file() {
            error(&format!("File {} not found", src));
        }

        if dst_path.exists() && !matches.opt_present("f") {
            error(&format!("File {} already exists", dst));
        }

        let verbose = matches.opt_present("v");
        let explicit_type = match matches.opt_str("t") {
            Some(value) => opts_archive_type(&value, verbose),
            None => INVALID,
        };
        let block_size = match matches.opt_str("b") {
            Some(value) => {
                match value.parse::<usize>() {
                    Ok(int) => int,
                    Err(_) => panic!(format!("Invalid block size: {}", value)),
                }
            }
            None => 1 << 24, // default to 16mb blocks
        };


        stream_file_into(src, dst, explicit_type, block_size, verbose);
    }
}
