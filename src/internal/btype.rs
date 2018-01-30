use std::io;

// ========================================================================= //

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockType {
    Verbatim,
    AlignedOffset,
    Uncompressed,
}

impl BlockType {
    pub fn from_bits(bits: u32) -> io::Result<BlockType> {
        match bits {
            1 => Ok(BlockType::Verbatim),
            2 => Ok(BlockType::AlignedOffset),
            3 => Ok(BlockType::Uncompressed),
            _ => invalid_data!("Invalid LZX block type ({})", bits),
        }
    }

    pub fn to_bits(&self) -> u32 {
        match *self {
            BlockType::Verbatim => 1,
            BlockType::AlignedOffset => 2,
            BlockType::Uncompressed => 3,
        }
    }
}

// ========================================================================= //


#[cfg(test)]
mod tests {
    use super::BlockType;

    #[test]
    #[should_panic(expected = "Invalid LZX block type (7)")]
    fn invalid_block_type() { BlockType::from_bits(7).unwrap(); }

    #[test]
    fn round_trip() {
        let btypes = &[
            BlockType::Verbatim,
            BlockType::AlignedOffset,
            BlockType::Uncompressed,
        ];
        for &btype in btypes {
            assert_eq!(BlockType::from_bits(btype.to_bits()).unwrap(), btype);
        }
    }
}

// ========================================================================= //
