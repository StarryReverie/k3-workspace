use proc_macro2::Ident;
use quote::quote;
use syn::{parse_macro_input, ItemTrait};

#[proc_macro_attribute]
pub fn core_lib_impl(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    // get the trait function list
    let trait_def = parse_macro_input!(item as ItemTrait);
    let func_list = trait_def.items.iter().filter_map(|item| {
        if let syn::TraitItem::Fn(method) = item {
            Some(method)
        } else {
            None
        }
    });
    // generate a function which name is the same as the trait function
    let func_impl = func_list.map(|method| {
        let func_attr = &method.attrs;
        let func_sig = &method.sig;
        let func_name = &func_sig.ident;
        let out_put = func_sig.output.clone();
        let inputs = &func_sig.inputs;
        let mut func_args = vec![];
        let input_argv = inputs
            .iter()
            .skip(1)
            .map(|arg| match arg {
                syn::FnArg::Typed(pat_type) => {
                    let pat = pat_type.pat.as_ref();
                    match pat {
                        syn::Pat::Ident(ident) => {
                            func_args.push(arg.clone());
                            let name = ident.ident.clone();
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
        quote!(
            #( #func_attr )*
            pub fn #func_name( #( #func_args ),* ) #out_put {
                CORE_FUNC.get_must().#func_name(#(#input_argv),* )
            }
        )
    });

    quote! (
        #trait_def
        #[macro_export]
        macro_rules! CORE_FUNC_GEN {
            () => {
                #(#func_impl)*
            }
        }
    )
    .into()
}
