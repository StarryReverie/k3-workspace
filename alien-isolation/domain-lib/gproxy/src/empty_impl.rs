use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{ItemTrait, TraitItem, TraitItemFn, TypeParamBound};

pub fn impl_empty_supertrait(ident: Ident, trait_def: ItemTrait) -> TokenStream {
    let supertraits = trait_def.supertraits.clone();
    let mut code = vec![];
    for supertrait in supertraits {
        if let TypeParamBound::Trait(trait_bound) = supertrait {
            let path = trait_bound.path.clone();
            let segments = path.segments;
            for segment in segments {
                let trait_name = segment.ident.clone();
                match trait_name.to_string().as_str() {
                    "DeviceBase" => {
                        let device_base = quote!(
                            impl DeviceBase for #ident{
                                fn handle_irq(&self)->AlienResult<()>{
                                    Err(AlienError::ENOSYS)
                                }
                            }
                        );
                        code.push(device_base)
                    }
                    "Basic" => {
                        let basic = quote!(
                            impl Basic for #ident{
                                fn is_active(&self)->bool{
                                    false
                                }
                                fn domain_id(&self)->u64{
                                    u64::MAX
                                }
                            }
                        );
                        code.push(basic)
                    }
                    _ => {}
                }
            }
        }
    }
    quote::quote!(
        #(#code)*
    )
}

pub fn impl_empty_func(func_vec: Vec<TraitItem>) -> Vec<TokenStream> {
    let mut func_codes = vec![];
    func_vec.iter().for_each(|item| match item {
        TraitItem::Fn(method) => {
            let func_code = impl_empty_func_code(method);
            func_codes.push(func_code);
        }
        _ => {
            panic!("item is not a function");
        }
    });
    func_codes
}

fn impl_empty_func_code(func: &TraitItemFn) -> TokenStream {
    let name = func.sig.ident.clone();
    let mut attr = func.attrs.clone();

    attr.retain(|attr| {
        let path = attr.path();
        !path.is_ident("recoverable") && !path.is_ident("no_check")
    });
    let mut sig = func.sig.clone();
    sig.inputs.iter_mut().skip(1).for_each(|arg| {
        match arg {
            syn::FnArg::Typed(pat_type) => {
                let pat = pat_type.pat.as_mut();
                match pat {
                    syn::Pat::Ident(ident) => {
                        let name = ident.ident.clone();
                        // name
                        ident.ident = Ident::new(&format!("_{}", name), name.span());
                    }
                    _ => {
                        panic!("not a ident");
                    }
                }
            }
            _ => {
                panic!("not a typed");
            }
        }
    }); // reset the input arguments

    match name.to_string().as_str() {
        "init" => {
            let token = quote!(
                #(#attr)*
                #sig{
                    Ok(())
                }
            );
            token
        }
        _ => {
            let token = quote!(
                #(#attr)*
                #sig{
                    Err(AlienError::ENOSYS)
                }
            );
            token
        }
    }
}

pub fn impl_empty_code(
    trait_name: &Ident,
    trait_def: ItemTrait,
) -> (Ident, TokenStream, TokenStream) {
    let func_vec = trait_def.items.clone();
    let empty_ident = Ident::new(&format!("{}EmptyImpl", trait_name), trait_name.span());
    let super_trait_empty_code = impl_empty_supertrait(empty_ident.clone(), trait_def);
    let empty_func_code = impl_empty_func(func_vec.clone());
    let def_code = quote!(
        #[derive(Debug)]
        struct #empty_ident;

        impl #empty_ident{
            pub fn new()->Self{
                Self
            }
        }
        #super_trait_empty_code
        impl #trait_name for #empty_ident{
            #(#empty_func_code)*
        }
    );
    let impl_empty_ident = Ident::new(&format!("impl_empty_for_{}", trait_name), trait_name.span());

    let impl_for_empty_code = quote!(
        #[macro_export]
        macro_rules! #impl_empty_ident {
            ($name:ident) => {
                impl #trait_name for $name{
                    #(#empty_func_code)*
                }
            }
        }
    );
    (empty_ident, def_code, impl_for_empty_code)
}
