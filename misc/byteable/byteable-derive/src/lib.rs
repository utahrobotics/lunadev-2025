use proc_macro::{self, TokenStream};
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(IntoBytes)]
pub fn describe(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, .. } = parse_macro_input!(input);

    let output = quote! {
        byteable::assert_impl_all!(#ident: byteable::IntoBytes);
        impl byteable::IntoBytes for #ident {
            fn to_bytes(&self) -> byteable::RecycleGuard<Vec<u8>> {
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
