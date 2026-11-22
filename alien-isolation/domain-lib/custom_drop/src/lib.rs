use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DataStruct, DeriveInput};

#[proc_macro_derive(CustomDrop)]
pub fn custom_drop(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    // call the custom_drop method for every field
    // of the struct
    let gen = match &input.data {
        syn::Data::Struct(DataStruct { fields, .. }) => {
            match fields {
                syn::Fields::Named(fields) => {
                    // named fields
                    let field_names = fields.named.iter().map(|field| {
                        let field_name = field.ident.as_ref().unwrap();
                        quote! {
                            self.#field_name.custom_drop();
                        }
                    });
                    quote! {
                        impl CustomDrop for #name {
                            fn custom_drop(&mut self) {
                                #(#field_names)*
                            }
                        }
                    }
                }
                syn::Fields::Unnamed(field) => {
                    // unnamed fields
                    let field_names = (0..field.unnamed.len()).map(|i| {
                        let field_name = syn::Index::from(i);
                        quote! {
                            self.#field_name.custom_drop();
                        }
                    });

                    quote! {
                        impl CustomDrop for #name {
                            fn custom_drop(&mut self) {
                                #(#field_names)*
                            }
                        }
                    }
                }
                syn::Fields::Unit => {
                    // unit struct
                    quote! {
                        impl CustomDrop for #name {
                            fn custom_drop(&mut self) {
                                // do nothing
                            }
                        }
                    }
                }
            }
        }
        syn::Data::Enum(_) => {
            panic!("Enums are not supported")
        }
        syn::Data::Union(_) => panic!("Unions are not supported"),
    };
    gen.into()
}
