//! A library for encoding/decoding
//! [LZXD](https://en.wikipedia.org/wiki/LZX_(algorithm)) compression streams,
//! such as those found in [Windows
//! cabinet](https://en.wikipedia.org/wiki/Cabinet_(file_format)) files.

#![warn(missing_docs)]

extern crate byteorder;

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Read};

// ========================================================================= //

/// The minimum permitted value for the `window` argument of `Decoder::new()`.
pub const WINDOW_MIN: u16 = 15;

/// The maximum permitted value for the `window` argument of `Decoder::new()`.
pub const WINDOW_MAX: u16 = 21;

const CHUNK_SIZE: u64 = 0x8000;

// ========================================================================= //

struct BitReader<R: Read> {
    reader: R,
    bit_buffer: u64,
    bits_in_buffer: u16,
    bits_mod_16: u16,
}

impl<R: Read> BitReader<R> {
    fn new(reader: R) -> BitReader<R> {
        BitReader {
            reader: reader,
            bit_buffer: 0,
            bits_in_buffer: 0,
            bits_mod_16: 0,
        }
    }

    fn ensure_buffer_has_at_least(&mut self, num_bits: u16) -> io::Result<()> {
        debug_assert!(num_bits <= 48);
        if self.bits_in_buffer < num_bits {
            let next = self.reader.read_u16::<LittleEndian>()? as u64;
            self.bit_buffer |= next << (48 - self.bits_in_buffer);
            self.bits_in_buffer += 16;
        }
        Ok(())
    }

    fn read_bits(&mut self, num_bits: u16) -> io::Result<u32> {
        debug_assert!(num_bits <= 32);
        self.ensure_buffer_has_at_least(num_bits)?;
        debug_assert!(self.bits_in_buffer >= num_bits);
        let bits = (self.bit_buffer >> (64 - num_bits)) as u32;
        self.bit_buffer <<= num_bits;
        self.bits_in_buffer -= num_bits;
        self.bits_mod_16 = (self.bits_mod_16 + num_bits) & 0xf;
        Ok(bits)
    }

    #[allow(dead_code)]
    fn peek_bits(&mut self, num_bits: u16) -> io::Result<u32> {
        debug_assert!(num_bits <= 32);
        self.ensure_buffer_has_at_least(num_bits)?;
        debug_assert!(self.bits_in_buffer >= num_bits);
        Ok((self.bit_buffer >> (64 - num_bits)) as u32)
    }

    fn align_to_16(&mut self) -> io::Result<()> {
        if self.bits_mod_16 > 0 {
            let bits_to_skip = 16 - self.bits_mod_16;
            self.read_bits(bits_to_skip)?;
        }
        debug_assert_eq!(self.bits_in_buffer & 0xf, 0);
        Ok(())
    }

    fn align_to_8(&mut self) -> io::Result<()> {
        let bits_mod_8 = self.bits_mod_16 & 0x7;
        if bits_mod_8 > 0 {
            self.read_bits(8 - bits_mod_8)?;
        }
        debug_assert_eq!(self.bits_in_buffer & 0x7, 0);
        Ok(())
    }
}

impl<R: Read> Read for BitReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.align_to_8()?;
        let mut bytes_read = 0;
        while self.bits_in_buffer > 0 && bytes_read < buf.len() {
            debug_assert!(self.bits_in_buffer >= 8);
            buf[bytes_read] = self.read_bits(8)? as u8;
        }
        if bytes_read < buf.len() {
            let num_bytes = self.reader.read(&mut buf[bytes_read..])?;
            if (num_bytes & 1) != 0 {
                self.bits_mod_16 ^= 8;
            }
            bytes_read += num_bytes;
        }
        Ok(bytes_read)
    }
}

// ========================================================================= //

#[derive(Clone, Copy, Eq, PartialEq)]
enum BlockType {
    Verbatim,
    AlignedOffset,
    Uncompressed,
}

impl BlockType {
    fn from_bits(bits: u32) -> io::Result<BlockType> {
        match bits {
            1 => Ok(BlockType::Verbatim),
            2 => Ok(BlockType::AlignedOffset),
            3 => Ok(BlockType::Uncompressed),
            _ => {
                let msg = format!("Invalid LZX block type ({})", bits);
                Err(io::Error::new(io::ErrorKind::InvalidData, msg))
            }
        }
    }
}

// ========================================================================= //

/// An LZXD decoder/decompressor.
///
/// Use the `Read` trait to read decompressed bytes from the `Decoder` stream.
#[allow(dead_code)]
pub struct Decoder<R: Read> {
    reader: BitReader<R>,
    total_uncompressed_bytes_remaining: u64,
    chunk_compressed_bytes_remaining: usize,
    chunk_uncompressed_bytes_remaining: usize,
    header_filesize: u32,
    block_type: BlockType,
    block_uncompressed_bytes_remaining: usize,
    recent: (u32, u32, u32),
    window: Vec<u8>,
}

impl<R: Read> Decoder<R> {
    /// Starts decoding an LZXD-compressed data stream.
    ///
    /// The `window` argument determines the size of the compression window,
    /// and its value must be between the `WINDOW_MIN` and `WINDOW_MAX`
    /// constants (inclusive).
    ///
    /// The `uncompressed_size` argument must specify the exact size of the of
    /// the original, uncompressed data, in bytes.
    pub fn new(mut reader: R, window: u16, uncompressed_size: u64)
               -> io::Result<Decoder<R>> {
        if window < WINDOW_MIN || window > WINDOW_MAX {
            let msg = format!("Invalid LZX window ({})", window);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        }
        let window_size: usize = 1 << window;
        let chunk_compressed_size = reader.read_u16::<LittleEndian>()? as
            usize;
        let chunk_uncompressed_size = CHUNK_SIZE.min(uncompressed_size) as
            usize;
        let mut decoder = Decoder {
            reader: BitReader::new(reader),
            total_uncompressed_bytes_remaining: uncompressed_size,
            chunk_compressed_bytes_remaining: chunk_compressed_size,
            chunk_uncompressed_bytes_remaining: chunk_uncompressed_size,
            header_filesize: 0,
            block_type: BlockType::Verbatim,
            block_uncompressed_bytes_remaining: 0,
            recent: (0, 0, 0),
            window: vec![0u8; window_size],
        };
        if decoder.reader.read_bits(1)? != 0 {
            decoder.header_filesize = decoder.reader.read_bits(32)?;
        }
        Ok(decoder)
    }
}

impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut bytes_read: usize = 0;
        while self.total_uncompressed_bytes_remaining > 0 &&
            bytes_read < buf.len()
        {
            if self.chunk_uncompressed_bytes_remaining == 0 {
                self.reader.align_to_16()?;
                self.chunk_compressed_bytes_remaining =
                    self.reader.read_u16::<LittleEndian>()? as usize;
                self.chunk_uncompressed_bytes_remaining =
                    CHUNK_SIZE.min(self.total_uncompressed_bytes_remaining) as
                        usize;
            }
            while self.block_uncompressed_bytes_remaining == 0 {
                if self.block_type == BlockType::Uncompressed {
                    self.reader.align_to_16()?;
                }
                self.block_type = BlockType::from_bits(self.reader
                                                           .read_bits(3)?)?;
                self.block_uncompressed_bytes_remaining =
                    self.reader.read_bits(24)? as usize;
                match self.block_type {
                    BlockType::Verbatim => unimplemented!(), // TODO
                    BlockType::AlignedOffset => unimplemented!(), // TODO
                    BlockType::Uncompressed => {
                        self.reader.read_bits(1)?;
                        self.reader.align_to_16()?;
                        self.recent.0 = self.reader
                            .read_u32::<LittleEndian>()?;
                        self.recent.1 = self.reader
                            .read_u32::<LittleEndian>()?;
                        self.recent.2 = self.reader
                            .read_u32::<LittleEndian>()?;
                    }
                }
            }
            let bytes_to_read = self.block_uncompressed_bytes_remaining
                .min(self.chunk_uncompressed_bytes_remaining)
                .min(buf.len() - bytes_read);
            debug_assert!(bytes_to_read > 0);
            match self.block_type {
                BlockType::Verbatim => unimplemented!(), // TODO
                BlockType::AlignedOffset => unimplemented!(), // TODO
                BlockType::Uncompressed => {
                    let end = bytes_read + bytes_to_read;
                    self.reader.read_exact(&mut buf[bytes_read..end])?;
                    bytes_read += bytes_to_read;
                    self.block_uncompressed_bytes_remaining -= bytes_to_read;
                    self.chunk_uncompressed_bytes_remaining -= bytes_to_read;
                    self.total_uncompressed_bytes_remaining -= bytes_to_read as
                        u64;
                }
            }
        }
        Ok(bytes_read)
    }
}

// ========================================================================= //

#[cfg(test)]
mod tests {
    use super::{BitReader, Decoder, WINDOW_MIN};
    use byteorder::{LittleEndian, ReadBytesExt};
    use std::io::Read;

    #[test]
    fn bit_reader() {
        let input: &[u8] = b"\xcd\xab\x80\x35\x34\x12";
        let mut reader = BitReader::new(input);
        assert_eq!(reader.read_u16::<LittleEndian>().unwrap(), 0xabcd);
        assert_eq!(reader.read_bits(1).unwrap(), 0);
        assert_eq!(reader.read_bits(3).unwrap(), 3);
        assert_eq!(reader.peek_bits(3).unwrap(), 2);
        assert_eq!(reader.read_bits(5).unwrap(), 11);
        assert_eq!(reader.read_u16::<LittleEndian>().unwrap(), 0x1234);
    }

    #[test]
    fn decode_stream_with_one_uncompressed_block() {
        let input: &[u8] = b"\x14\x00\x00\x30\x30\x00\x01\x00\x00\x00\x01\
            \x00\x00\x00\x01\x00\x00\x00\x61\x62\x63\x00";
        let mut decoder = Decoder::new(input, WINDOW_MIN, 3).unwrap();
        let mut buffer = [0u8; 10];
        assert_eq!(decoder.read(&mut buffer).unwrap(), 3);
        assert_eq!(&buffer[..3], b"abc");
    }
}

// ========================================================================= //
