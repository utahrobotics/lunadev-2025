use proc_macro::{self, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(IntoBytes)]
pub fn into_bytes(input: TokenStream) -> TokenStream {
    let DeriveInput {
        ident, generics, ..
    } = parse_macro_input!(input);
    if !generics.params.is_empty() {
        return syn::Error::new_spanned(generics, "generic types are not supported")
            .to_compile_error()
            .into();
    }

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::FillByteVec);
        impl byteable::IntoBytes for #ident {
            fn to_bytes(&self) -> byteable::RecycleGuard<Vec<u8>> {
                use byteable::FillByteVec;

                thread_local! {
                    static RECYCLER: byteable::Recycler<Vec<u8>> = byteable::Recycler::default();
                }

                RECYCLER.with(|recycler| {
                    let mut bytes = recycler.get_or_else(|| Vec::with_capacity(#ident::SIZE_HINT));
                    bytes.clear();
                    self.fill_bytes((&mut *bytes).try_into().unwrap());
                    bytes
                })
            }
        }
    };

    output.into()
}

#[proc_macro_derive(IntoBytesBincode)]
pub fn into_bytes_bincode(input: TokenStream) -> TokenStream {
    let input2 = input.clone();
    let DeriveInput {
        ident, generics, ..
    } = parse_macro_input!(input2);
    if !generics.params.is_empty() {
        return syn::Error::new_spanned(generics, "generic types are not supported")
            .to_compile_error()
            .into();
    }

    let additional = proc_macro2::TokenStream::from(into_bytes(input));

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::serde::Serialize);

        impl byteable::FillByteVec for #ident {
            const SIZE_HINT: usize = 0;

            fn fill_bytes(&self, vec: byteable::EmptyVec<u8>) {
                let vec: &mut Vec<u8> = vec.into();
                byteable::bincode::serialize_into(vec, self).expect("Failed to serialize");
            }
        }

        #additional
    };

    output.into()
}

#[proc_macro_derive(FillByteVecBitcode)]
pub fn fill_byte_vec_bitcode(input: TokenStream) -> TokenStream {
    let input2 = input.clone();
    let DeriveInput {
        ident, generics, ..
    } = parse_macro_input!(input2);
    if !generics.params.is_empty() {
        return syn::Error::new_spanned(generics, "generic types are not supported")
            .to_compile_error()
            .into();
    }

    let buffer_ident = format_ident!("__{ident}_BUFFER");

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::bitcode::Encode);

        thread_local! {
            #[allow(non_upper_case_globals)]
            static #buffer_ident: std::cell::RefCell<std::collections::VecDeque<byteable::bitcode::Buffer>> = std::cell::RefCell::default();
        }

        impl byteable::FillByteVec for #ident {
            const SIZE_HINT: usize = 0;

            fn fill_bytes(&self, vec: byteable::EmptyVec<u8>) {
                let vec: &mut Vec<u8> = vec.into();
                #buffer_ident.with_borrow_mut(|queue| {
                    if queue.is_empty() {
                        queue.push_back(Default::default());
                    }
                    let buf = queue.front_mut().unwrap();
                    vec.extend_from_slice(buf.encode(self));
                });
            }
        }
    };

    output.into()
}

#[proc_macro_derive(IntoBytesSlice)]
pub fn into_bytes_slice(input: TokenStream) -> TokenStream {
    let input2 = input.clone();
    let DeriveInput {
        ident, generics, ..
    } = parse_macro_input!(input2);
    if !generics.params.is_empty() {
        return syn::Error::new_spanned(generics, "generic types are not supported")
            .to_compile_error()
            .into();
    }

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::IntoBytes);
        impl byteable::IntoBytesSlice for #ident {
            fn into_bytes_slice<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
                f(&self.to_bytes())
            }
        }
    };

    output.into()
}

#[proc_macro_derive(IntoBytesSliceBitcode)]
pub fn into_bytes_slice_bitcode(input: TokenStream) -> TokenStream {
    let input2 = input.clone();
    let DeriveInput {
        ident, generics, ..
    } = parse_macro_input!(input2);
    if !generics.params.is_empty() {
        return syn::Error::new_spanned(generics, "generic types are not supported")
            .to_compile_error()
            .into();
    }

    let buffer_ident = format_ident!("__{ident}_BUFFER");

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::bitcode::Encode);

        impl byteable::IntoBytesSlice for #ident {
            fn into_bytes_slice<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
                let mut buf = #buffer_ident.with_borrow_mut(|queue| {
                    queue.pop_front().unwrap_or_default()
                });
                let result = f(buf.encode(self));
                #buffer_ident.with_borrow_mut(|queue| {
                    queue.push_back(buf);
                });
                result
            }
        }
    };

    output.into()
}
