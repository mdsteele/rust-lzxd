//! A library for encoding/decoding
//! [LZXD](https://en.wikipedia.org/wiki/LZX_(algorithm)) compression streams,
//! such as those found in [Windows
//! cabinet](https://en.wikipedia.org/wiki/Cabinet_(file_format)) files.

#![warn(missing_docs)]

extern crate byteorder;

mod internal;

pub use internal::consts::{WINDOW_MAX, WINDOW_MIN};
pub use internal::decoder::Decoder;
pub use internal::encoder::Encoder;

// ========================================================================= //
