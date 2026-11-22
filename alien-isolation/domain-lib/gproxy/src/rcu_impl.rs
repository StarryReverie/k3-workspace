use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{ItemTrait, TraitItem, TraitItemFn};

use crate::{
    common::{
        collect_func_info, gen_trampoline_info, resource_code, FuncInfo, ResourceCode,
        TrampolineArg, TrampolineInfo,
    },
    empty_impl::impl_empty_code,
    super_trait::impl_supertrait,
    unwind_impl::impl_unwind_code,
    Proxy, SyncType,
};

pub fn def_struct_rcu(proxy: Proxy, trait_def: ItemTrait) -> TokenStream {
    let trait_name = &trait_def.ident;
    let func_vec = trait_def.items.clone();

    let ident = proxy.ident.clone();
    let super_trait_code = impl_supertrait(ident.clone(), trait_def.clone(), SyncType::Srcu);

    let (func_code, extern_func_code) =
        impl_func(func_vec, trait_name, &ident, proxy.source.is_some());

    let macro_ident = Ident::new(&format!("gen_for_{}", trait_name), trait_name.span());
    let impl_ident = Ident::new(&format!("impl_for_{}", trait_name), trait_name.span());

    let (empty_ident, empty_def_code, empty_impl_for_code) =
        impl_empty_code(trait_name, trait_def.clone());

    let (_, unwind_def, unwind_impl_for) = impl_unwind_code(trait_name, trait_def.clone());

    let ResourceCode {
        resource_field,
        resource_init,
        cast,
        call_once,
        replace_call,
    } = resource_code(&proxy);

    let prox_ext_impl = impl_prox_ext_trait(&ident, replace_call, trait_name);

    quote::quote!(
        #[macro_export]
        macro_rules! #macro_ident {
            () => {
                #[derive(Debug)]
                pub struct #ident{
                    domain: RcuData<Box<dyn #trait_name>>,
                    domain_loader: Mutex<DomainLoader>,
                    #resource_field
                }
                impl #ident{
                    pub fn new(domain: Box<dyn #trait_name>,domain_loader: DomainLoader)->Self{
                        Self{
                            domain: RcuData::new(Box::new(domain)),
                            domain_loader: Mutex::new(domain_loader),
                            #resource_init
                        }
                    }
                    pub fn domain_loader(&self) -> DomainLoader{
                        self.domain_loader.lock().clone()
                    }
                }

                impl ProxyBuilder for #ident{
                    type T = Box<dyn #trait_name>;
                    fn build(domain: Self::T,domain_loader: DomainLoader)->Self{
                        Self::new(domain,domain_loader)
                    }
                    fn build_empty(domain_loader: DomainLoader)->Self{
                        let domain = Box::new(#empty_ident::new());
                        Self::new(domain,domain_loader)
                    }
                    fn init_by_box(&self, argv: Box<dyn Any+Send+Sync>) -> AlienResult<()>{
                        #cast
                        #call_once
                        Ok(())
                    }
                }

                #super_trait_code


                impl #trait_name for #ident{
                    #(#func_code)*
                }

                #prox_ext_impl

                #(#extern_func_code)*

                #empty_def_code

            };
        }

        #[macro_export]
        macro_rules! #impl_ident {
            ($name:ident) => {
                impl #trait_name for $name{
                    #(#func_code)*
                }
            }
        }
        #empty_impl_for_code

        #unwind_def
        #unwind_impl_for
    )
}

fn impl_prox_ext_trait(
    proxy_name: &Ident,
    replace_call: TokenStream,
    trait_name: &Ident,
) -> TokenStream {
    quote!(
        impl #proxy_name{
             pub fn replace(&self,new_domain: Box<dyn #trait_name>,loader:DomainLoader) -> AlienResult<()> {
                let total = TimeTick::new("Total Time");
                let mut loader_guard = self.domain_loader.lock();
                let old_id = self.domain_id();
                let tick = TimeTick::new("Reinit domain without state");
                // init the new domain before swap
                #replace_call
                drop(tick);

               let old_domain = self.domain.update(Box::new(new_domain));

                let tick = TimeTick::new("Recycle resources");
                // forget the old domain
                // it will be dropped by the `free_domain_resource`
                let real_domain = Box::into_inner(old_domain);
                core::mem::forget(real_domain);
                free_domain_resource(old_id, FreeShared::Free,free_frames);
                drop(tick);
                *loader_guard = loader;
                Ok(())
            }
        }
    )
}

fn impl_func(
    func_vec: Vec<TraitItem>,
    trait_name: &Ident,
    proxy_name: &Ident,
    has_resource: bool,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let mut func_codes = vec![];
    let extern_func_codes = vec![];
    func_vec.iter().for_each(|item| match item {
        TraitItem::Fn(method) => {
            let func_code = impl_func_code(method, trait_name, proxy_name, has_resource);
            func_codes.push(func_code);
        }
        _ => {
            panic!("item is not a function");
        }
    });
    (func_codes, extern_func_codes)
}

fn impl_func_code(
    func: &TraitItemFn,
    trait_name: &Ident,
    proxy_name: &Ident,
    _has_resource: bool,
) -> TokenStream {
    let FuncInfo {
        has_recovery,
        no_check,
        func_name,
        attr,
        sig,
        input_argv,
        output,
        fn_args,
        arg_domain_change,
    } = collect_func_info(func);

    match func_name.to_string().as_str() {
        "init" => {
            if !input_argv.is_empty() {
                assert_eq!(input_argv.len(), 1);
            }
            let token = quote!(
                #(#attr)*
                #sig{
                    self.domain.read_directly(|domain|{
                        domain.#func_name(#(#input_argv),*)
                    })
                }
            );
            token
        }
        _ => {
            let func_inner = gen_trampoline(TrampolineArg {
                has_recovery,
                trait_name,
                proxy_name,
                func_name,
                input_argv,
                fn_args,
                arg_domain_change,
                out_put: output,
                no_check,
            });

            let token = quote!(
                #(#attr)*
                #sig{
                    #func_inner
                }
            );
            token
        }
    }
}

fn gen_trampoline(arg: TrampolineArg) -> TokenStream {
    let TrampolineArg {
        has_recovery: _has_recovery,
        trait_name: _trait_name,
        proxy_name: _proxy_name,
        func_name,
        input_argv,
        fn_args: _fn_args,
        arg_domain_change,
        out_put: _out_put,
        no_check,
    } = arg;

    let TrampolineInfo {
        get_domain_id,
        check_code,
        call_move_to,
    } = gen_trampoline_info(&arg_domain_change, no_check);

    quote! (
            #check_code
            self.domain.read(|domain|{
                #get_domain_id
                #(#arg_domain_change)*
                domain.#func_name(#(#input_argv),*).map(|r| {
                    #call_move_to
                    r
                })
            })
    )
}
