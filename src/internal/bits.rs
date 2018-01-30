use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Read, Write};

// ========================================================================= //

pub struct BitReader<R: Read> {
    reader: R,
    bit_buffer: u64,
    bits_in_buffer: u16,
    bits_mod_16: u16,
}

impl<R: Read> BitReader<R> {
    pub fn new(reader: R) -> BitReader<R> {
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

    pub fn read_bits(&mut self, num_bits: u16) -> io::Result<u32> {
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

    pub fn align_to_16(&mut self) -> io::Result<()> {
        if self.bits_mod_16 != 0 {
            let bits_to_skip = 16 - self.bits_mod_16;
            self.read_bits(bits_to_skip)?;
        }
        debug_assert_eq!(self.bits_in_buffer & 0xf, 0);
        Ok(())
    }

    fn align_to_8(&mut self) -> io::Result<()> {
        let bits_mod_8 = self.bits_mod_16 & 0x7;
        if bits_mod_8 != 0 {
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
        while self.bits_in_buffer != 0 && bytes_read < buf.len() {
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

pub struct BitWriter<W: Write> {
    writer: W,
    bit_buffer: u64,
    bits_in_buffer: u16,
    extra_byte: bool,
}

impl<W: Write> BitWriter<W> {
    pub fn new(writer: W) -> BitWriter<W> {
        BitWriter {
            writer: writer,
            bit_buffer: 0,
            bits_in_buffer: 0,
            extra_byte: false,
        }
    }

    fn purge_bit_buffer(&mut self) -> io::Result<()> {
        debug_assert!(self.bits_in_buffer < 16);
        if self.bits_in_buffer != 0 {
            let bits_to_fill = 16 - self.bits_in_buffer;
            self.write_bits(bits_to_fill, 0)?;
        }
        debug_assert_eq!(self.bits_in_buffer, 0);
        Ok(())
    }

    fn fill_extra_byte(&mut self) -> io::Result<()> {
        if self.extra_byte {
            self.writer.write_u8(0)?;
            self.extra_byte = false;
        }
        Ok(())
    }

    pub fn write_bits(&mut self, num_bits: u16, bits: u32) -> io::Result<()> {
        self.fill_extra_byte()?;
        debug_assert!(num_bits <= 32);
        debug_assert!(self.bits_in_buffer < 16);
        debug_assert_eq!(bits >> num_bits, 0);
        self.bit_buffer |= (bits as u64) <<
            (64 - num_bits - self.bits_in_buffer);
        self.bits_in_buffer += num_bits;
        while self.bits_in_buffer >= 16 {
            let next = (self.bit_buffer >> 48) as u16;
            self.writer.write_u16::<LittleEndian>(next)?;
            self.bit_buffer <<= 16;
            self.bits_in_buffer -= 16;
        }
        Ok(())
    }

    pub fn align_to_16(&mut self) -> io::Result<()> {
        self.purge_bit_buffer()?;
        self.fill_extra_byte()?;
        Ok(())
    }
}

impl<W: Write> Write for BitWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.purge_bit_buffer()?;
        let num_bytes = self.writer.write(buf)?;
        if (num_bytes & 1) != 0 {
            self.extra_byte = !self.extra_byte;
        }
        Ok(num_bytes)
    }

    fn flush(&mut self) -> io::Result<()> { self.writer.flush() }
}

// ========================================================================= //

#[cfg(test)]
mod tests {
    use super::{BitReader, BitWriter};
    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

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
    fn bit_writer() {
        let mut output = Vec::<u8>::new();
        {
            let mut writer = BitWriter::new(&mut output);
            writer.write_u16::<LittleEndian>(0xabcd).unwrap();
            writer.write_bits(1, 0).unwrap();
            writer.write_bits(3, 3).unwrap();
            writer.write_bits(5, 11).unwrap();
            writer.write_u16::<LittleEndian>(0x1234).unwrap();
        }
        let expected: &[u8] = b"\xcd\xab\x80\x35\x34\x12";
        assert_eq!(output.as_slice(), expected);
    }
}

// ========================================================================= //
