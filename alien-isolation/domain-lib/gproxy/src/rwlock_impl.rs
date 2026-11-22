use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{FnArg, ItemTrait, ReturnType, TraitItem, TraitItemFn};

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

pub fn def_struct_rwlock(proxy: Proxy, trait_def: ItemTrait) -> TokenStream {
    let trait_name = &trait_def.ident;
    let func_vec = trait_def.items.clone();

    let ident = proxy.ident.clone();
    let super_trait_code = impl_supertrait(ident.clone(), trait_def.clone(), SyncType::Rwlock);

    let (func_code, other) = impl_func(func_vec, trait_name, &ident, proxy.source.is_some());

    let extern_func_code = other[0].clone();
    let inner_call_code = other[1].clone();

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

    // let ident_key = Ident::new(
    //     &format!("{}_KEY", ident.to_string().to_uppercase()),
    //     trait_name.span(),
    // );

    let prox_ext_impl = impl_prox_ext_trait(&ident, replace_call, trait_name);

    quote::quote!(
        #[macro_export]
        macro_rules! #macro_ident {
            () => {
                #[derive(Debug)]
                pub struct #ident{
                    domain: RcuData<Box<dyn #trait_name>>,
                    lock: RwLock<()>,
                    domain_loader: SleepMutex<DomainLoader>,
                    flag: core::sync::atomic::AtomicBool,
                    counter: PerCpuCounter,
                    #resource_field
                }
                impl #ident{
                    pub fn new(domain: Box<dyn #trait_name>,domain_loader: DomainLoader)->Self{
                        Self{
                            domain: RcuData::new(Box::new(domain)),
                            lock: RwLock::new(()),
                            domain_loader: SleepMutex::new(domain_loader),
                            flag: core::sync::atomic::AtomicBool::new(false),
                            counter: PerCpuCounter::new(),
                            #resource_init
                        }
                    }

                    pub fn all_counter(&self) -> usize {
                        self.counter.all()
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

                impl #ident{
                    #(#inner_call_code)*
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
                impl $name{
                    #(#inner_call_code)*
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
    let code = quote!(
        impl #proxy_name{
            pub fn replace(&self,new_domain: Box<dyn #trait_name>,loader:DomainLoader) -> AlienResult<()> {
                // stage1: get the sleep lock and change to updating state
                let mut loader_guard = self.domain_loader.lock();
                let old_id = self.domain_id();

                let tick = TimeTick::new("Task Sync");
                 // stage2: get the write lock and wait for all readers to finish
                let w_lock = self.lock.write();

                self.flag.store(true, core::sync::atomic::Ordering::SeqCst);

                // why we need to synchronize_sched here?
                sync_cpus();

                // wait if there are readers which are reading the old domain but no read lock
                while self.all_counter() > 0 {
                    // println!("Wait for all reader to finish");
                    // yield_now();
                }
                drop(tick);

                let tick = TimeTick::new("Reinit and state transfer");

                // stage3: init the new domain before swap
                let new_domain_id = new_domain.domain_id();
                #replace_call
                drop(tick);

                let tick = TimeTick::new("Domain swap");
                // stage4: swap the domain and change to normal state
                let old_domain = self.domain.update_directly(Box::new(new_domain));
                // change to normal state
                self.flag.store(false, core::sync::atomic::Ordering::SeqCst);
                drop(tick);

                let tick = TimeTick::new("Recycle resources");
                // stage5: recycle all resources
                let real_domain = Box::into_inner(old_domain);
                core::mem::forget(real_domain);

                free_domain_resource(old_id, FreeShared::NotFree(new_domain_id),free_frames);
                drop(tick);

                // stage6: release all locks
                *loader_guard = loader;
                drop(w_lock);
                drop(loader_guard);
                Ok(())
            }
        }
    );
    code
}

fn impl_func(
    func_vec: Vec<TraitItem>,
    trait_name: &Ident,
    proxy_name: &Ident,
    has_resource: bool,
) -> (Vec<TokenStream>, Vec<Vec<TokenStream>>) {
    let mut func_codes = vec![];
    let mut extern_func_codes = vec![vec![], vec![]];
    func_vec.iter().for_each(|item| match item {
        TraitItem::Fn(method) => {
            let (func_code, inner_call_code) =
                impl_func_code_rwlock(method, trait_name, proxy_name, has_resource);
            func_codes.push(func_code);
            extern_func_codes[1].push(inner_call_code);
        }
        _ => {
            panic!("item is not a function");
        }
    });
    (func_codes, extern_func_codes)
}

fn impl_func_code_rwlock(
    func: &TraitItemFn,
    trait_name: &Ident,
    proxy_name: &Ident,
    _has_resource: bool,
) -> (TokenStream, TokenStream) {
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
                        domain.init(#(#input_argv),*)
                    })
                }
            );
            (token, quote!())
        }
        _ => {
            let (func_inner, inner_call) = gen_trampoline_rwlock(TrampolineArg {
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
            (token, inner_call)
        }
    }
}

fn gen_trampoline_rwlock(arg: TrampolineArg) -> (TokenStream, TokenStream) {
    let TrampolineArg {
        has_recovery,
        trait_name,
        proxy_name: _,
        func_name,
        input_argv,
        fn_args,
        arg_domain_change,
        out_put,
        no_check,
    } = arg;

    let info = gen_trampoline_info(&arg_domain_change, no_check);

    let (inner_call_code, __ident_no_lock, __ident_with_lock) = impl_inner_code(
        has_recovery,
        (&func_name, trait_name),
        &fn_args,
        &input_argv,
        out_put,
        &arg_domain_change,
        &info,
    );

    // let ident_key = Ident::new(
    //     &format!("{}_KEY", proxy_name.to_string().to_uppercase()),
    //     proxy_name.span(),
    // );
    let call = quote!(
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            return self.#__ident_with_lock(#(#input_argv),*);
        }
        self.#__ident_no_lock(#(#input_argv),*)
    );
    // println!("{:?}",real_code.to_string());
    (call, inner_call_code)
}

fn impl_inner_code(
    _has_recover: bool,
    func_trait_name: (&Ident, &Ident),
    fn_argv: &Vec<FnArg>,
    input_argv: &Vec<Ident>,
    output: ReturnType,
    arg_domain_change: &Vec<TokenStream>,
    info: &TrampolineInfo,
) -> (TokenStream, Ident, Ident) {
    let (func_name, _trait_name) = func_trait_name;
    let __ident = Ident::new(&format!("__{}", func_name), func_name.span());
    let __ident_no_lock = Ident::new(&format!("__{}_no_lock", func_name), func_name.span());
    let __ident_with_lock = Ident::new(&format!("__{}_with_lock", func_name), func_name.span());

    let TrampolineInfo {
        get_domain_id,
        check_code,
        call_move_to,
    } = info;

    let ident_call = quote!(
        // let r_domain = self.domain.get();
        // #check_code
        // #get_domain_id
        // #(#arg_domain_change)*
        // let res = r_domain.#func_name(#(#input_argv),*).map(|r| {
        //     #call_move_to
        //     r
        // });
        // res
        self.domain.read_directly(|domain|{
            #check_code
            #get_domain_id
            #(#arg_domain_change)*
            domain.#func_name(#(#input_argv),*).map(|r| {
                #call_move_to
                r
            })
        })
    );

    let inner_call = quote!(
        #[inline(always)]
        fn #__ident(&self, #(#fn_argv),*)#output{
            #ident_call
        }
        #[inline(always)]
        fn #__ident_no_lock(&self, #(#fn_argv),*)#output{
            self.counter.inc();
            let res = self.#__ident(#(#input_argv),*);
            self.counter.dec();
            res
        }
        #[cold]
        #[inline(always)]
        fn #__ident_with_lock(&self, #(#fn_argv),*)#output{
            // let r_lock = self.lock.read();
            let r_lock = loop {
                if let Some(r) = self.lock.try_read() {
                    break r;
                }else {
                    yield_now();
                }
            };
            let res = self.#__ident(#(#input_argv),*);
            drop(r_lock);
            res
        }
    );

    (inner_call, __ident_no_lock, __ident_with_lock)
}
