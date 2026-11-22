use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, FnArg, ReturnType, Signature, TraitItemFn};

use crate::Proxy;

pub struct ResourceCode {
    pub resource_field: TokenStream,
    pub resource_init: TokenStream,
    pub cast: TokenStream,
    pub call_once: TokenStream,
    pub replace_call: TokenStream,
}

pub fn resource_code(proxy: &Proxy) -> ResourceCode {
    let resource_field = if proxy.source.is_some() {
        quote! (
            resource: Once<Box<dyn Any+Send+Sync>>
        )
    } else {
        quote!()
    };
    let (resource_init, cast, call_once, replace_call) = if proxy.source.is_some() {
        let s1 = quote! (
            resource: Once::new()
        );
        let s_ty = proxy.source.as_ref().unwrap();
        let s2 = quote! (
            let arg = argv.as_ref().downcast_ref::<#s_ty>().unwrap();
            self.init(arg)?;
        );
        let s3 = quote! (
            self.resource.call_once(|| argv);
        );

        let s4 = quote! (
            let resource = self.resource.get().unwrap();
            let info = resource.as_ref().downcast_ref::<#s_ty>().unwrap();
            new_domain.init(info).unwrap();
        );

        (s1, s2, s3, s4)
    } else {
        let s2 = quote!(
            let _ = argv;
            self.init()?;
        );
        let s4 = quote! (
            new_domain.init().unwrap();
        );
        (quote!(), s2, quote!(), s4)
    };
    ResourceCode {
        resource_field,
        resource_init,
        cast,
        call_once,
        replace_call,
    }
}

pub struct FuncInfo {
    pub has_recovery: bool,
    pub no_check: bool,
    pub func_name: Ident,
    pub attr: Vec<Attribute>,
    pub sig: Signature,
    pub input_argv: Vec<Ident>,
    pub output: ReturnType,
    pub fn_args: Vec<FnArg>,
    pub arg_domain_change: Vec<TokenStream>,
}

pub fn collect_func_info(func: &TraitItemFn) -> FuncInfo {
    let has_recover = func.attrs.iter().any(|attr| {
        let path = attr.path();
        path.is_ident("recoverable")
    });
    let no_check = func.attrs.iter().any(|attr| {
        let path = attr.path();
        path.is_ident("no_check")
    });

    let name = func.sig.ident.clone();
    let mut attr = func.attrs.clone();

    attr.retain(|attr| {
        let path = attr.path();
        !path.is_ident("recoverable") && !path.is_ident("no_check")
    });

    let sig = func.sig.clone();
    let input = sig.inputs.clone();
    let out_put = sig.output.clone();
    let mut fn_args = vec![];

    let mut arg_domain_change = vec![];

    let input_argv = input
        .iter()
        .skip(1)
        .map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                let ty = pat_type.ty.as_ref().to_token_stream().to_string();
                let pat = pat_type.pat.as_ref();
                match pat {
                    syn::Pat::Ident(ident) => {
                        fn_args.push(arg.clone());
                        let name = ident.ident.clone();
                        if ty.starts_with("DBox") || ty.starts_with("DVec") {
                            let change_code = quote!(
                                let old_id = #name.move_to(id);
                            );
                            arg_domain_change.push(change_code);
                        }
                        name
                    }
                    _ => {
                        panic!("not a ident");
                    }
                }
            }
            _ => {
                panic!("not a typed");
            }
        })
        .collect::<Vec<Ident>>();
    FuncInfo {
        has_recovery: has_recover,
        no_check,
        func_name: name,
        attr,
        sig,
        input_argv,
        output: out_put,
        fn_args,
        arg_domain_change,
    }
}

pub struct TrampolineInfo {
    pub get_domain_id: TokenStream,
    pub check_code: TokenStream,
    pub call_move_to: TokenStream,
}

pub struct TrampolineArg<'a> {
    pub has_recovery: bool,
    pub trait_name: &'a Ident,
    pub proxy_name: &'a Ident,
    pub func_name: Ident,
    pub input_argv: Vec<Ident>,
    pub fn_args: Vec<FnArg>,
    pub arg_domain_change: Vec<TokenStream>,
    pub out_put: ReturnType,
    pub no_check: bool,
}
pub fn gen_trampoline_info(arg_domain_change: &[TokenStream], no_check: bool) -> TrampolineInfo {
    let get_domain_id = if arg_domain_change.is_empty() {
        quote!()
    } else {
        let x1 = quote!(
            let id = domain.domain_id();
        );
        x1
    };
    let check_code = if no_check {
        quote!()
    } else {
        // quote!(if !r_domain.is_active() {
        //     return Err(AlienError::DOMAINCRASH);
        // })
        quote!()
    };

    let call_move_to = if arg_domain_change.is_empty() {
        let x2 = quote!();
        x2
    } else {
        let x2 = quote!(
            r.move_to(old_id);
        );
        x2
    };

    TrampolineInfo {
        get_domain_id,
        check_code,
        call_move_to,
    }
}
