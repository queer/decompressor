use std::io::{self, BufRead, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use color_eyre::eyre::Result;

fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let lock = Arc::new(AtomicBool::new(false));
    let wait_lock = lock.clone();

    thread::spawn(move || {
        thread::sleep(Duration::from_secs(5));
        if !wait_lock.load(Ordering::SeqCst) {
            eprintln!();
            eprintln!("No data received after 5 seconds, aborting.");
            std::process::exit(1);
        }
    });

    let mut buffer = [0; 6];
    let n = stdin.read(&mut buffer)?;
    let buffer = &buffer[..n];
    let mut stream = buffer.chain(stdin);

    let compression_type = detect_compression_type(&buffer);

    lock.store(true, Ordering::SeqCst);

    if compression_type == CompressionType::Lzma {
        // lzma-rs doesn't support streaming decompression, so we have to use a different
        // library for that eventually...
        decompress_lzma(&mut stream)?;
    } else {
        decompress(&mut stream, compression_type)?;
    }

    Ok(())
}

fn decompress_lzma(reader: &mut impl BufRead) -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    lzma_rs::lzma_decompress(reader, &mut stdout)?;
    Ok(())
}

fn decompress(reader: &mut impl Read, compression_type: CompressionType) -> Result<()> {
    match compression_type {
        CompressionType::Zstd => {
            let mut decoder = zstd::stream::Decoder::new(reader)?;
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::Brotli => {
            let mut decoder = brotli::Decompressor::new(reader, 4096);
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::Gzip => {
            let mut decoder = flate2::read::GzDecoder::new(reader);
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::Deflate => {
            let mut decoder = flate2::read::DeflateDecoder::new(reader);
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::Zlib => {
            let mut decoder = flate2::read::ZlibDecoder::new(reader);
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::Xz => {
            let mut decoder = xz2::read::XzDecoder::new(reader);
            write_to_stdout(&mut decoder)?;
        }
        CompressionType::None => {
            eprintln!("c: hint: no compression detected, writing directly to stdout");
            eprintln!("c: hint: brotli detection isn't possible without decompressing");
            eprintln!("c: hint: use `brotli` as the first argument to force brotli detection if this isn't plain text");
            
            write_to_stdout(reader)?;
        }
        _ => {}
    };
    Ok(())
}

fn write_to_stdout(reader: &mut impl Read) -> Result<u64> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    io::copy(reader, &mut stdout).map_err(|e| e.into())
}

fn detect_compression_type(buffer: &[u8]) -> CompressionType {
    let hint = std::env::args()
        .nth(1)
        .map(|s| s.to_lowercase())
        .or(Some("unknown".into()))
        .unwrap();

    if buffer.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        CompressionType::Zstd
    } else if buffer.starts_with(&[0x1f, 0x8b]) {
        CompressionType::Gzip
    } else if buffer.starts_with(&[0x78, 0x01]) {
        CompressionType::Deflate
    } else if buffer.starts_with(&[0x78, 0x9c]) {
        CompressionType::Zlib
    } else if buffer.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        CompressionType::Xz
    } else if buffer.starts_with(&[0x5d, 0x00]) {
        CompressionType::Lzma
    } else if "brotli" == hint {
        CompressionType::Brotli
    } else {
        CompressionType::None
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CompressionType {
    Zstd,
    Brotli,
    Gzip,
    Deflate,
    Zlib,
    Lzma,
    Xz,
    None,
}
