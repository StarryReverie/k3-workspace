mod common;
mod empty_impl;
mod rcu_impl;
mod rwlock_impl;
mod super_trait;
mod unwind_impl;

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, ItemTrait, Token, Type,
};

use crate::{rcu_impl::def_struct_rcu, rwlock_impl::def_struct_rwlock};

enum SyncType {
    Srcu,
    Rwlock,
}

struct Proxy {
    ident: Ident,
    sync: Ident,
    source: Option<Type>,
}

impl Parse for Proxy {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let sync: Ident = input.parse()?;
        if sync != "SRCU" && sync != "RwLock" {
            return Err(syn::Error::new(
                sync.span(),
                "sync type must be SRCU or RwLock",
            ));
        }
        let comma: Option<Token![,]> = input.parse()?;
        match comma {
            Some(_) => {
                let ty = input.parse::<Type>()?;
                Ok(Proxy {
                    ident,
                    sync,
                    source: Some(ty),
                })
            }
            None => Ok(Proxy {
                ident,
                sync,
                source: None,
            }),
        }
    }
}

#[proc_macro_attribute]
/// Whether to generate trampoline code for the function
pub fn recoverable(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let item = TokenStream::from(item);
    quote! (
        #item
    )
    .into()
}
#[proc_macro_attribute]
/// Do not check if the domain is active
pub fn no_check(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let item = TokenStream::from(item);
    quote! (
        #item
    )
    .into()
}

#[proc_macro_attribute]
pub fn proxy(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let proxy = parse_macro_input!(attr as Proxy);
    let trait_def = parse_macro_input!(item as ItemTrait);
    let struct_def = if proxy.sync == "SRCU" {
        def_struct_rcu(proxy, trait_def.clone())
    } else {
        def_struct_rwlock(proxy, trait_def.clone())
    };
    quote!(
        #trait_def
        #struct_def
    )
    .into()
}
