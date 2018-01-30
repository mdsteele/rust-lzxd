use byteorder::{LittleEndian, WriteBytesExt};
use internal::bits::BitWriter;
use internal::btype::BlockType;
use internal::consts;
use std::io::{self, Write};

// ========================================================================= //

/// An LZXD encoder/compressor.
pub struct Encoder<W: Write> {
    writer: BitWriter<W>,
    wrote_header: bool,
    total_uncompressed_bytes_remaining: u64,
    chunk_buffer: Vec<u8>,
}

impl<W: Write> Encoder<W> {
    /// Starts encoding an LZXD-compressed data stream.
    ///
    /// The `window` argument determines the size of the compression window,
    /// and its value must be between the `WINDOW_MIN` and `WINDOW_MAX`
    /// constants (inclusive).
    ///
    /// The `uncompressed_size` argument must specify the exact size of the of
    /// the original, uncompressed data, in bytes.
    pub fn new(writer: W, window: u16, uncompressed_size: u64)
               -> io::Result<Encoder<W>> {
        if window < consts::WINDOW_MIN || window > consts::WINDOW_MAX {
            invalid_input!("Invalid LZX window ({})", window);
        }
        let encoder = Encoder {
            writer: BitWriter::new(writer),
            wrote_header: false,
            total_uncompressed_bytes_remaining: uncompressed_size,
            chunk_buffer: Vec::with_capacity(consts::CHUNK_SIZE),
        };
        Ok(encoder)
    }

    fn emit_chunk(&mut self) -> io::Result<()> {
        debug_assert!(!self.chunk_buffer.is_empty());
        debug_assert!(self.chunk_buffer.len() <= consts::CHUNK_SIZE);
        debug_assert!(self.chunk_buffer.len() == consts::CHUNK_SIZE ||
                          self.total_uncompressed_bytes_remaining == 0);
        // TODO: Don't always produce uncompressed blocks.
        let chunk_compressed_size = 16 + self.chunk_buffer.len() +
            (self.chunk_buffer.len() & 1);
        self.writer.align_to_16()?;
        self.writer.write_u16::<LittleEndian>(chunk_compressed_size as u16)?;
        if !self.wrote_header {
            self.writer.write_bits(1, 0)?;
            self.wrote_header = true;
        }
        self.writer.write_bits(3, BlockType::Uncompressed.to_bits())?;
        self.writer.write_bits(24, self.chunk_buffer.len() as u32)?;
        self.writer.write_bits(1, 0)?;
        self.writer.align_to_16()?;
        self.writer.write_u32::<LittleEndian>(1)?; // R0
        self.writer.write_u32::<LittleEndian>(1)?; // R1
        self.writer.write_u32::<LittleEndian>(1)?; // R2
        self.writer.write_all(&self.chunk_buffer)?;
        self.writer.align_to_16()?;
        self.chunk_buffer.clear();
        debug_assert_eq!(self.chunk_buffer.capacity(), consts::CHUNK_SIZE);
        Ok(())
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut bytes_written = 0;
        while self.total_uncompressed_bytes_remaining > 0 &&
            bytes_written < buf.len()
        {
            debug_assert!(self.chunk_buffer.len() < consts::CHUNK_SIZE);
            let num_bytes =
                (self.total_uncompressed_bytes_remaining
                     .min((consts::CHUNK_SIZE - self.chunk_buffer.len()) as
                              u64) as usize)
                    .min(buf.len() - bytes_written);
            let end = bytes_written + num_bytes;
            self.chunk_buffer.write_all(&buf[bytes_written..end])?;
            debug_assert!(self.chunk_buffer.len() <= consts::CHUNK_SIZE);
            bytes_written += num_bytes;
            self.total_uncompressed_bytes_remaining -= num_bytes as u64;
            if self.chunk_buffer.len() == consts::CHUNK_SIZE {
                self.emit_chunk()?;
            }
        }
        if self.total_uncompressed_bytes_remaining == 0 &&
            !self.chunk_buffer.is_empty()
        {
            self.emit_chunk()?;
        }
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> { self.writer.flush() }
}

// ========================================================================= //

#[cfg(test)]
mod tests {
    use super::Encoder;
    use internal::consts::WINDOW_MIN;
    use std::io::Write;

    #[test]
    #[should_panic(expected = "Invalid LZX window (12345)")]
    fn invalid_window_size() {
        let mut output = Vec::<u8>::new();
        Encoder::new(&mut output, 12345, 3).unwrap();
    }

    #[test]
    fn encode_tiny_stream() {
        let mut output = Vec::<u8>::new();
        {
            let mut encoder = Encoder::new(&mut output, WINDOW_MIN, 3)
                .unwrap();
            encoder.write_all(b"abc").unwrap();
        }
        let expected: &[u8] =
            b"\x14\x00\x00\x30\x30\x00\x01\x00\x00\x00\x01\
              \x00\x00\x00\x01\x00\x00\x00\x61\x62\x63\x00";
        assert_eq!(output.as_slice(), expected);
    }
}

// ========================================================================= //
