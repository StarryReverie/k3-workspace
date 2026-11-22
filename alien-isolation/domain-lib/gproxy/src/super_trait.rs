use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{ItemTrait, TypeParamBound};

use crate::SyncType;

pub fn impl_supertrait(ident: Ident, trait_def: ItemTrait, sync_ty: SyncType) -> TokenStream {
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
                        let (ext_code, inner_code) = match sync_ty {
                            SyncType::Srcu => (quote!(), impl_srcu_code()),
                            SyncType::Rwlock => (impl_lock_code(&ident), impl_rwlock_code(&ident)),
                        };

                        let device_base = quote!(
                            #ext_code
                            impl DeviceBase for #ident{
                                fn handle_irq(&self)->AlienResult<()>{
                                    #inner_code
                                }
                            }
                        );
                        code.push(device_base)
                    }
                    "Basic" => {
                        let (ext_code, inner_code) = match sync_ty {
                            SyncType::Srcu => (quote!(), srcu_for_domain_id()),
                            SyncType::Rwlock => {
                                (lock_for_domain_id(&ident), rwlock_for_domain_id())
                            }
                        };
                        let basic = quote!(
                            #ext_code
                            impl Basic for #ident{
                                fn domain_id(&self)->u64{
                                    #inner_code
                                }
                                fn is_active(&self)->bool{
                                    true
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

fn srcu_for_domain_id() -> TokenStream {
    quote!(self.domain.read(|domain| domain.domain_id()))
}

fn rwlock_for_domain_id() -> TokenStream {
    quote!(
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            return self.__domain_id_with_lock();
        }
        self.__domain_id_no_lock()
    )
}

fn lock_for_domain_id(ident: &Ident) -> TokenStream {
    quote!(
        impl #ident{
            fn __domain_id(&self)->u64{
                self.domain.read_directly(|domain|domain.domain_id())
            }
            fn __domain_id_no_lock(&self)->u64{
                self.counter.inc();
                let r = self.__domain_id();
                self.counter.dec();
                r
            }
            fn __domain_id_with_lock(&self)->u64{
                let lock = self.lock.read();
                let r = self.__domain_id();
                drop(lock);
                r
            }
        }
    )
}

fn impl_srcu_code() -> TokenStream {
    quote!(self.domain.read(|domain| domain.handle_irq()))
}

fn impl_rwlock_code(_ident: &Ident) -> TokenStream {
    quote!(
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            return self.__handle_irq_with_lock();
        }
        self.__handle_irq_no_lock()
    )
}
fn impl_lock_code(ident: &Ident) -> TokenStream {
    quote!(
        impl #ident{
            fn __handle_irq(&self) -> AlienResult<()> {
                self.domain.read_directly(|domain|domain.handle_irq())
            }
            fn __handle_irq_no_lock(&self) -> AlienResult<()> {
                self.counter.inc();
                let res = self.__handle_irq();
                self.counter.dec();
                res
            }
            #[cold]
            fn __handle_irq_with_lock(&self) -> AlienResult<()> {
                let r_lock = self.lock.read();
                let res = self.__handle_irq();
                drop(r_lock);
                res
            }
        }
    )
}
