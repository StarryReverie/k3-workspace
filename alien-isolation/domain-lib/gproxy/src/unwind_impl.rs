use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{ItemTrait, TraitItem, TraitItemFn, TypeParamBound};

use crate::common::{collect_func_info, FuncInfo};
pub fn impl_unwind_supertrait(ident: Ident, trait_def: ItemTrait) -> TokenStream {
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
                                    basic::catch_unwind(||{
                                        self.0.handle_irq()
                                    })
                                    // self.0.handle_irq()
                                }
                            }
                        );
                        code.push(device_base)
                    }
                    "Basic" => {
                        let basic = quote!(
                            impl Basic for #ident{
                                fn is_active(&self)->bool{
                                    self.0.is_active()
                                }
                                fn domain_id(&self)->u64{
                                    self.0.domain_id()
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

pub fn impl_unwind_func(func_vec: Vec<TraitItem>) -> Vec<TokenStream> {
    let mut func_codes = vec![];
    func_vec.iter().for_each(|item| match item {
        TraitItem::Fn(method) => {
            let func_code = impl_unwind_func_code(method);
            func_codes.push(func_code);
        }
        _ => {
            panic!("item is not a function");
        }
    });
    func_codes
}

fn impl_unwind_func_code(func: &TraitItemFn) -> TokenStream {
    let FuncInfo {
        has_recovery: _has_recovery,
        no_check: _no_check,
        func_name: _func_name,
        attr: _attr,
        sig,
        input_argv,
        output: _,
        fn_args: _,
        arg_domain_change: _,
    } = collect_func_info(func);
    let name = func.sig.ident.clone();
    let mut attr = func.attrs.clone();

    attr.retain(|attr| {
        let path = attr.path();
        !path.is_ident("recoverable") && !path.is_ident("no_check")
    });

    match name.to_string().as_str() {
        "init" => {
            let token = quote!(
                #(#attr)*
                #sig{
                    self.0.init(#(#input_argv),*)
                }
            );
            token
        }
        _ => {
            let token = quote!(
                #(#attr)*
                #sig{
                    basic::catch_unwind(||{
                        self.0.#name(#(#input_argv),*)
                    })
                    // self.0.#name(#(#input_argv),*)
                }
            );
            token
        }
    }
}

pub fn impl_unwind_code(
    trait_name: &Ident,
    trait_def: ItemTrait,
) -> (Ident, TokenStream, TokenStream) {
    let func_vec = trait_def.items.clone();
    let unwind_ident = Ident::new(&"UnwindWrap".to_string(), trait_name.span());
    let super_trait_empty_code = impl_unwind_supertrait(unwind_ident.clone(), trait_def);
    let unwind_func_code = impl_unwind_func(func_vec.clone());

    let define_unwind_macro = Ident::new(
        &format!("define_unwind_for_{}", trait_name),
        trait_name.span(),
    );

    let define_unwind_code = quote! (
        #[macro_export]
        macro_rules! #define_unwind_macro {
            ($name:ident) => {
                #[derive(Debug)]
                pub struct #unwind_ident($name);
                impl #unwind_ident{
                    pub fn new(real:$name)->Self{
                        Self(real)
                    }
                }

                #super_trait_empty_code
                impl #trait_name for #unwind_ident{
                    #(#unwind_func_code)*
                }
            }
        }
    );

    let impl_unwind_ident = Ident::new(
        &format!("impl_unwind_for_{}", trait_name),
        trait_name.span(),
    );

    let impl_for_unwind_code = quote!(
        #[macro_export]
        macro_rules! #impl_unwind_ident {
            ($name:ident) => {
                impl #trait_name for $name{
                    #(#unwind_func_code)*
                }
            }
        }
    );
    (unwind_ident, define_unwind_code, impl_for_unwind_code)
}
