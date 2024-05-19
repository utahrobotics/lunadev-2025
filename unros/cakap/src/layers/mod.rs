pub mod error_correction;

pub trait Layer {
    const IDENTIFIER: &'static [u8];
}
