#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
use alloc::{boxed::Box, collections::VecDeque, sync::Arc};

use basic::{
    config::MAX_INPUT_EVENT_NUM,
    println,
    sync::{Mutex, Once, OnceGet},
    AlienError, AlienResult,
};
use interface::{
    define_unwind_for_BufInputDomain, Basic, BufInputDomain, DeviceBase, DomainType, InputDomain,
};

#[derive(Debug)]
struct BufInput {
    max_events: u32,
    inner: Mutex<BufInputInner>,
    input: Once<Arc<dyn InputDomain>>,
}

#[derive(Debug)]
struct BufInputInner {
    events: VecDeque<u64>,
    wait_queue: VecDeque<usize>,
}

impl BufInput {
    pub fn new(max_events: u32) -> Self {
        let inner = BufInputInner {
            events: VecDeque::with_capacity(max_events as usize),
            wait_queue: VecDeque::new(),
        };
        BufInput {
            max_events,
            inner: Mutex::new(inner),
            input: Once::new(),
        }
    }
}

impl DeviceBase for BufInput {
    fn handle_irq(&self) -> AlienResult<()> {
        let mut inner = self.inner.lock();
        let input_domain = self.input.get_must();
        let mut count = 0;
        while let Some(event) = input_domain.event_nonblock()? {
            // info!("event: {:?}", event);
            if inner.events.len() >= self.max_events as usize {
                // remove the first event
                inner.events.pop_front();
            }
            inner.events.push_back(event);
            count += 1;
        }
        while !inner.wait_queue.is_empty() && count > 0 {
            let tid = inner.wait_queue.pop_front().unwrap();
            basic::wake_up_wait_task(tid)?;
            count -= 1;
        }
        // info!("read {} events", count);
        Ok(())
    }
}

impl Basic for BufInput {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl BufInputDomain for BufInput {
    fn init(&self, input_domain_name: &str) -> AlienResult<()> {
        let input_domain = basic::get_domain(input_domain_name).unwrap();
        match input_domain {
            DomainType::InputDomain(input) => {
                self.input.call_once(|| input);
                Ok(())
            }
            ty => {
                println!(
                    "find input domain by name {},ty: {:?}",
                    input_domain_name, ty
                );
                Err(AlienError::EINVAL)
            }
        }?;
        Ok(())
    }

    fn event_block(&self) -> AlienResult<u64> {
        loop {
            let mut inner = self.inner.lock();
            if let Some(event) = inner.events.pop_front() {
                return Ok(event);
            }
            let tid = basic::current_tid()?.unwrap();
            inner.wait_queue.push_back(tid);
            drop(inner);
            basic::wait_now()?;
        }
    }

    fn event_nonblock(&self) -> AlienResult<Option<u64>> {
        let mut inner = self.inner.lock();
        Ok(inner.events.pop_front())
    }

    fn have_event(&self) -> AlienResult<bool> {
        let inner = self.inner.lock();
        Ok(!inner.events.is_empty())
    }
}

define_unwind_for_BufInputDomain!(BufInput);

pub fn main() -> Box<dyn BufInputDomain> {
    Box::new(UnwindWrap::new(BufInput::new(MAX_INPUT_EVENT_NUM)))
}
