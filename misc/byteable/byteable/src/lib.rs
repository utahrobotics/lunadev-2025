pub use byteable_core::*;
#[cfg(feature = "derive")]
pub use byteable_derive::*;
pub use static_assertions::assert_impl_all;

#[cfg(feature = "bincode")]
mod bincode_impl {
    use byteable_core::FillByteVec;

    pub struct BincodeWrapper<T, const N: usize = 0>(pub T);

    impl<T: serde::Serialize, const N: usize> FillByteVec for BincodeWrapper<T, N> {
        const SIZE_HINT: usize = N;

        fn fill_bytes(&self, vec: byteable_core::EmptyVec<u8>) {
            let vec: &mut Vec<u8> = vec.into();
            bincode::serialize_into(vec, &self.0).expect("Failed to serialize");
        }
    }
}

#[cfg(feature = "bitcode")]
mod bitcode_impl {
    use std::cell::RefCell;

    use bitcode::Buffer;
    use byteable_core::FillByteVec;

    pub struct BitcodeWrapper<T, const N: usize = 0>(pub T);

    thread_local! {
        static BUFFER: RefCell<Buffer> = RefCell::new(Buffer::new());
    }

    impl<T: bitcode::Encode, const N: usize> FillByteVec for BitcodeWrapper<T, N> {
        const SIZE_HINT: usize = N;

        fn fill_bytes(&self, vec: byteable_core::EmptyVec<u8>) {
            let vec: &mut Vec<u8> = vec.into();
            BUFFER.with_borrow_mut(|buf| {
                vec.extend_from_slice(buf.encode(&self.0));
            });
        }
    }
}
