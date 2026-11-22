use alloc::boxed::Box;
use core::{any::Any, fmt::Debug, mem::forget, sync::atomic::AtomicBool};

use interface::*;
use ksync::RwLock;
use mem::free_frames;
use shared_heap::{DBox, SharedData};
use task_meta::TaskSchedulingInfo;

use crate::{
    domain_helper::{free_domain_resource, FreeShared},
    domain_loader::loader::DomainLoader,
    domain_proxy::{PerCpuCounter, ProxyBuilder},
    error::{AlienError, AlienResult},
    sync::{sync_cpus, RcuData, SleepMutex},
    timer::TimeTick,
};

#[derive(Debug)]
pub struct SchedulerDomainProxy {
    domain: RcuData<Box<dyn SchedulerDomain>>,
    lock: RwLock<()>,
    domain_loader: SleepMutex<DomainLoader>,
    flag: AtomicBool,
    counter: PerCpuCounter,
}
impl SchedulerDomainProxy {
    pub fn new(domain: Box<dyn SchedulerDomain>, domain_loader: DomainLoader) -> Self {
        Self {
            domain: RcuData::new(Box::new(domain)),
            lock: RwLock::new(()),
            domain_loader: SleepMutex::new(domain_loader),
            flag: AtomicBool::new(false),
            counter: PerCpuCounter::new(),
        }
    }
    pub fn all_counter(&self) -> usize {
        self.counter.all()
    }
}
impl ProxyBuilder for SchedulerDomainProxy {
    type T = Box<dyn SchedulerDomain>;
    fn build(domain: Self::T, domain_loader: DomainLoader) -> Self {
        Self::new(domain, domain_loader)
    }
    fn build_empty(domain_loader: DomainLoader) -> Self {
        let domain = Box::new(SchedulerDomainEmptyImpl::new());
        Self::new(domain, domain_loader)
    }
    fn init_by_box(&self, argv: Box<dyn Any + Send + Sync>) -> AlienResult<()> {
        let _ = argv;
        self.init()?;
        Ok(())
    }
}
impl Basic for SchedulerDomainProxy {
    fn domain_id(&self) -> u64 {
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            self.__domain_id_with_lock()
        } else {
            self.__domain_id_no_lock()
        }
    }
    fn is_active(&self) -> bool {
        true
    }
}
impl SchedulerDomain for SchedulerDomainProxy {
    fn init(&self) -> AlienResult<()> {
        self.domain.read_directly(|domain| domain.init())
    }
    fn add_task(&self, scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()> {
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            return self.__add_task_with_lock(scheduling_info);
        }
        self.__add_task_no_lock(scheduling_info)
    }
    fn fetch_task(&self, info: DBox<TaskSchedulingInfo>) -> AlienResult<DBox<TaskSchedulingInfo>> {
        if self.flag.load(core::sync::atomic::Ordering::SeqCst) {
            return self.__fetch_task_with_lock(info);
        }
        self.__fetch_task_no_lock(info)
    }
}
impl SchedulerDomainProxy {
    fn __domain_id(&self) -> u64 {
        self.domain.read_directly(|domain| domain.domain_id())
    }

    fn __domain_id_no_lock(&self) -> u64 {
        self.counter.inc();
        let r = self.__domain_id();
        self.counter.dec();
        r
    }
    fn __domain_id_with_lock(&self) -> u64 {
        let lock = self.lock.read();
        let r = self.__domain_id();
        drop(lock);
        r
    }

    fn __add_task(&self, scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()> {
        self.domain.read_directly(|domain| {
            let id = domain.domain_id();
            let old_id = scheduling_info.move_to(id);
            domain.add_task(scheduling_info).map(|r| {
                r.move_to(old_id);
                r
            })
        })
    }
    fn __add_task_no_lock(&self, scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()> {
        self.counter.inc();
        let res = self.__add_task(scheduling_info);
        self.counter.dec();
        res
    }
    #[cold]
    fn __add_task_with_lock(&self, scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()> {
        let r_lock = self.lock.read();
        let res = self.__add_task(scheduling_info);
        drop(r_lock);
        res
    }
    fn __fetch_task(
        &self,
        info: DBox<TaskSchedulingInfo>,
    ) -> AlienResult<DBox<TaskSchedulingInfo>> {
        self.domain.read_directly(|domain| {
            let id = domain.domain_id();
            let old_id = info.move_to(id);
            domain.fetch_task(info).map(|r| {
                r.move_to(old_id);
                r
            })
        })
    }
    fn __fetch_task_no_lock(
        &self,
        info: DBox<TaskSchedulingInfo>,
    ) -> AlienResult<DBox<TaskSchedulingInfo>> {
        self.counter.inc();
        let res = self.__fetch_task(info);
        self.counter.dec();
        res
    }
    #[cold]
    fn __fetch_task_with_lock(
        &self,
        info: DBox<TaskSchedulingInfo>,
    ) -> AlienResult<DBox<TaskSchedulingInfo>> {
        let r_lock = self.lock.read();
        let res = self.__fetch_task(info);
        drop(r_lock);
        res
    }
}

impl SchedulerDomainProxy {
    pub fn replace(
        &self,
        new_domain: Box<dyn SchedulerDomain>,
        loader: DomainLoader,
    ) -> AlienResult<()> {
        // stage1: get the sleep lock and change to updating state
        let mut loader_guard = self.domain_loader.lock();
        let old_id = self.domain_id();

        let tick = TimeTick::new("Task Sync");
        self.flag.store(true, core::sync::atomic::Ordering::SeqCst);

        // why we need to synchronize_sched here?
        sync_cpus();

        // stage2: get the write lock and wait for all readers to finish
        let w_lock = self.lock.write();
        // wait if there are readers which are reading the old domain but no read lock
        // todo!( "wait for all reader to finish");
        while self.all_counter() > 0 {
            // println!(
            //     "Wait for all reader to finish: {}",
            //     self.all_counter() as isize
            // );
        }
        drop(tick);

        let tick = TimeTick::new("Reinit and state transfer");

        // stage3: init the new domain before swap
        let new_domain_id = new_domain.domain_id();
        new_domain.init().unwrap();

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
        // forget the old domain, it will be dropped by the `free_domain_resource`
        forget(real_domain);

        // todo!(how to recycle the old domain)
        // We should not free the shared data here, because the shared data will be used
        // in new domain.
        free_domain_resource(old_id, FreeShared::NotFree(new_domain_id), free_frames);
        drop(tick);

        // stage6: release all locks
        *loader_guard = loader;
        drop(w_lock);
        drop(loader_guard);
        Ok(())
    }
}

#[derive(Debug)]
struct SchedulerDomainEmptyImpl;
impl SchedulerDomainEmptyImpl {
    pub fn new() -> Self {
        Self
    }
}
impl Basic for SchedulerDomainEmptyImpl {
    fn domain_id(&self) -> u64 {
        u64::MAX
    }
    fn is_active(&self) -> bool {
        false
    }
}
impl SchedulerDomain for SchedulerDomainEmptyImpl {
    fn init(&self) -> AlienResult<()> {
        Ok(())
    }
    #[doc = " add one task to scheduler"]
    fn add_task(&self, _scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()> {
        Err(AlienError::ENOSYS)
    }
    #[doc = " The next task to run"]
    fn fetch_task(&self, _info: DBox<TaskSchedulingInfo>) -> AlienResult<DBox<TaskSchedulingInfo>> {
        Err(AlienError::ENOSYS)
    }
}
