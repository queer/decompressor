use std::io::{self, BufRead, Read, Write};

use color_eyre::eyre::Result;

use crate::Flags;

const BROTLI_BUFFER_SIZE: usize = 4096;
const BROTLI_Q: u32 = 42;
const BROTLI_LGWIN: u32 = 69;

const XZ_LEVEL: u32 = 6;

const ZSTD_LEVEL: i32 = 6;

pub fn detect_stream_characteristics<R: Read>(
    stream: &mut R,
    flags: &Flags,
) -> Result<(CompressionType, Vec<u8>)> {
    let mut buffer = [0; 6];
    let n = stream.read(&mut buffer)?;
    let buffer = &buffer[..n];
    let kind = detect_compression_type(buffer, flags);

    Ok((kind, Vec::from(buffer)))
}

fn detect_compression_type(buffer: &[u8], flags: &Flags) -> CompressionType {
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
    }
    /*else if buffer.starts_with(&[0x5d, 0x00]) {
        CompressionType::Lzma
    } */
    else if "brotli" == flags.hint {
        CompressionType::Brotli
    } else {
        CompressionType::None
    }
}

pub struct Context<'a, R: Read, W: Write> {
    input_compression_type: CompressionType,
    output_compression_type: CompressionType,

    input_stream: Box<&'a mut R>,
    output_stream: Box<&'a mut W>,
}

impl<'a, R: Read, W: Write> Context<'a, R, W> {
    pub fn new_from_stream(
        input_stream: &'a mut R,
        output_stream: &'a mut W,
        input_compression_type: CompressionType,
        flags: &Flags,
    ) -> Result<Self> {
        Ok(Self {
            input_compression_type,
            output_compression_type: flags.output_type.unwrap_or(CompressionType::None),
            input_stream: Box::new(input_stream),
            output_stream: Box::new(output_stream),
        })
    }

    pub fn translate_stream(&mut self) -> Result<()> {
        let input_stream = self.input_stream.as_mut();
        let output_stream = self.output_stream.as_mut();

        let mut decompressor: Box<dyn Decompressor> = match self.input_compression_type {
            CompressionType::Zstd => {
                let decoder = zstd::Decoder::new(input_stream)?;
                Box::new(ZstdDecompressor(decoder))
            }
            CompressionType::Brotli => {
                let decoder = brotli::Decompressor::new(input_stream, 4096);
                Box::new(BrotliDecompressor(decoder))
            }
            CompressionType::Gzip => {
                let decoder = flate2::read::GzDecoder::new(input_stream);
                Box::new(GzipDecompressor(decoder))
            }
            CompressionType::Deflate => {
                let decoder = flate2::read::DeflateDecoder::new(input_stream);
                Box::new(DeflateDecompressor(decoder))
            }
            CompressionType::Zlib => {
                let decoder = flate2::read::ZlibDecoder::new(input_stream);
                Box::new(ZlibDecompressor(decoder))
            }
            // CompressionType::Lzma => {
            //     let decoder = lzma_rs::lzma_decompressor::LzmaDecompressor::new(stream);
            //     LzmaDecompressor(decoder)
            // }
            CompressionType::Xz => {
                let decoder = xz2::read::XzDecoder::new(input_stream);
                Box::new(XzDecompressor(decoder))
            }
            CompressionType::None => {
                let decoder = input_stream;
                Box::new(NoneDecompressor(decoder))
            }
        };

        let mut compressor: Box<dyn Compressor> = match self.output_compression_type {
            CompressionType::Zstd => {
                let encoder = zstd::Encoder::new(output_stream, ZSTD_LEVEL)?.auto_finish();
                Box::new(ZstdCompressor(encoder))
            }
            CompressionType::Brotli => {
                let encoder = brotli::CompressorWriter::new(
                    output_stream,
                    BROTLI_BUFFER_SIZE,
                    BROTLI_Q,
                    BROTLI_LGWIN,
                );
                Box::new(BrotliCompressor(encoder))
            }
            CompressionType::Gzip => {
                let encoder =
                    flate2::write::GzEncoder::new(output_stream, flate2::Compression::default());
                Box::new(GzipCompressor(encoder))
            }
            CompressionType::Deflate => {
                let encoder = flate2::write::DeflateEncoder::new(
                    output_stream,
                    flate2::Compression::default(),
                );
                Box::new(DeflateCompressor(encoder))
            }
            CompressionType::Zlib => {
                let encoder =
                    flate2::write::ZlibEncoder::new(output_stream, flate2::Compression::default());
                Box::new(ZlibCompressor(encoder))
            }
            // CompressionType::Lzma => {
            //     let encoder = lzma_rs::lzma_compress::LzmaCompressor::new(output_stream, 6);
            //     LzmaCompressor(encoder)
            // }
            CompressionType::Xz => {
                let encoder = xz2::write::XzEncoder::new(output_stream, XZ_LEVEL);
                Box::new(XzCompressor(encoder))
            }
            CompressionType::None => {
                let encoder = output_stream;
                Box::new(NoneCompressor(encoder))
            }
        };

        io::copy(&mut decompressor, &mut compressor)?;

        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum, strum::Display)]
pub enum CompressionType {
    None,
    Brotli,
    Deflate,
    Gzip,
    Xz,
    Zlib,
    Zstd,
    // Lzma,
}

// Compression //

trait Compressor: Write {
    fn compress(&mut self, stream: Box<dyn Read>) -> Result<()>;
}

struct ZstdCompressor<'a, T: Write>(zstd::stream::write::AutoFinishEncoder<'a, T>);

impl<T: Write> Write for ZstdCompressor<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for ZstdCompressor<'_, T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct BrotliCompressor<T: Write>(brotli::CompressorWriter<T>);

impl<T: Write> Write for BrotliCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for BrotliCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct GzipCompressor<T: Write>(flate2::write::GzEncoder<T>);

impl<T: Write> Write for GzipCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for GzipCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct DeflateCompressor<T: Write>(flate2::write::DeflateEncoder<T>);

impl<T: Write> Write for DeflateCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for DeflateCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct ZlibCompressor<T: Write>(flate2::write::ZlibEncoder<T>);

impl<T: Write> Write for ZlibCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for ZlibCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct XzCompressor<T: Write>(xz2::write::XzEncoder<T>);

impl<T: Write> Write for XzCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Compressor for XzCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

struct NoneCompressor<T: Write>(T);

impl<T: Write> Compressor for NoneCompressor<T> {
    fn compress(&mut self, mut stream: Box<dyn Read>) -> Result<()> {
        io::copy(&mut stream, &mut self.0)?;
        Ok(())
    }
}

impl<T: Write> Write for NoneCompressor<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

// Decompression //

trait Decompressor: Read {
    fn decompress(&mut self, stream: Box<dyn Write>) -> Result<()>;
}

struct ZstdDecompressor<'a, T: BufRead>(zstd::Decoder<'a, T>);

impl<T: BufRead> Read for ZstdDecompressor<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: BufRead> Decompressor for ZstdDecompressor<'_, T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct BrotliDecompressor<T: Read>(brotli::Decompressor<T>);

impl<T: Read> Read for BrotliDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for BrotliDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct GzipDecompressor<T: Read>(flate2::read::GzDecoder<T>);

impl<T: Read> Read for GzipDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for GzipDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct DeflateDecompressor<T: Read>(flate2::read::DeflateDecoder<T>);

impl<T: Read> Read for DeflateDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for DeflateDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct ZlibDecompressor<T: Read>(flate2::read::ZlibDecoder<T>);

impl<T: Read> Read for ZlibDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for ZlibDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct XzDecompressor<T: Read>(xz2::read::XzDecoder<T>);

impl<T: Read> Read for XzDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for XzDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

struct NoneDecompressor<T: Read>(T);

impl<T: Read> Read for NoneDecompressor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: Read> Decompressor for NoneDecompressor<T> {
    fn decompress(&mut self, mut stream: Box<dyn Write>) -> Result<()> {
        io::copy(&mut self.0, &mut stream)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;

    use super::*;
    use color_eyre::Result;

    #[test]
    fn test_none_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "none".into(),
                output_type: Some(CompressionType::None),
            },
        )?;

        ctx.translate_stream()?;

        assert_eq!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_zstd_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "zstd".into(),
                output_type: Some(CompressionType::Zstd),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let mut encoder = zstd::Encoder::new(&mut compressed_stream, ZSTD_LEVEL)?.auto_finish();
            encoder.write_all(expected.as_bytes())?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_brotli_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "brotli".into(),
                output_type: Some(CompressionType::Brotli),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(
                &mut compressed_stream,
                BROTLI_BUFFER_SIZE,
                BROTLI_Q,
                BROTLI_LGWIN,
            );
            encoder.write_all(expected.as_bytes())?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_gzip_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "gzip".into(),
                output_type: Some(CompressionType::Gzip),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let encoder = flate2::write::GzEncoder::new(
                &mut compressed_stream,
                flate2::Compression::default(),
            );
            let mut compressor = GzipCompressor(encoder);
            compressor.compress(Box::new(expected.as_bytes()))?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_deflate_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "deflate".into(),
                output_type: Some(CompressionType::Deflate),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let encoder = flate2::write::DeflateEncoder::new(
                &mut compressed_stream,
                flate2::Compression::default(),
            );
            let mut compressor = DeflateCompressor(encoder);
            compressor.compress(Box::new(expected.as_bytes()))?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_zlib_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "zlib".into(),
                output_type: Some(CompressionType::Zlib),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let encoder = flate2::write::ZlibEncoder::new(
                &mut compressed_stream,
                flate2::Compression::default(),
            );
            let mut compressor = ZlibCompressor(encoder);
            compressor.compress(Box::new(expected.as_bytes()))?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }

    #[test]
    fn test_xz_compression_works() -> Result<()> {
        let expected = "this is a test";
        let mut input_stream = expected.as_bytes();
        let mut output_stream: Vec<u8> = Vec::new();

        let mut ctx = Context::new_from_stream(
            &mut input_stream,
            &mut output_stream,
            CompressionType::None,
            &crate::Flags {
                quiet: true,
                hint: "xz".into(),
                output_type: Some(CompressionType::Xz),
            },
        )?;

        ctx.translate_stream()?;

        let mut compressed_stream: Vec<u8> = Vec::new();
        {
            let mut encoder = xz2::write::XzEncoder::new(&mut compressed_stream, XZ_LEVEL);
            encoder.write_all(expected.as_bytes())?;
        }

        assert!(!compressed_stream.is_empty());
        assert_eq!(compressed_stream, output_stream);
        assert_ne!(expected.as_bytes(), output_stream);

        Ok(())
    }
}
