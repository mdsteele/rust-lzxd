use byteorder::{LittleEndian, ReadBytesExt};
use internal::bits::BitReader;
use internal::btype::BlockType;
use internal::consts;
use std::io::{self, Read};

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
        if window < consts::WINDOW_MIN || window > consts::WINDOW_MAX {
            invalid_input!("Invalid LZX window ({})", window);
        }
        let window_size: usize = 1 << window;
        let chunk_compressed_size = reader.read_u16::<LittleEndian>()? as
            usize;
        let chunk_uncompressed_size =
            uncompressed_size.min(consts::CHUNK_SIZE as u64) as usize;
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
                    self.total_uncompressed_bytes_remaining
                        .min(consts::CHUNK_SIZE as u64) as
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
    use super::Decoder;
    use internal::consts::WINDOW_MIN;
    use std::io::Read;

    #[test]
    #[should_panic(expected = "Invalid LZX window (12345)")]
    fn invalid_window_size() {
        let input: &[u8] = b"\x14\x00\x00\x30\x30\x00\x01\x00\x00\x00\x01\
            \x00\x00\x00\x01\x00\x00\x00\x61\x62\x63\x00";
        Decoder::new(input, 12345, 3).unwrap();
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
