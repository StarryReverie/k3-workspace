use alloc::{collections::VecDeque, sync::Arc};
use core::{fmt::Debug, sync::atomic::AtomicU32};

use basic::{
    constants::{
        epoll::EventFdFlags,
        io::{PollEvents, SeekFrom},
    },
    sync::Mutex,
    AlienError, AlienResult,
};
use shared_heap::DVec;
use vfscore::{dentry::VfsDentry, inode::VfsInode, utils::VfsFileStat};

use crate::kfile::File;

static EVENTFD_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct EventFd {
    count: u64,
    flags: EventFdFlags,
    #[allow(unused)]
    id: u32,
}

impl EventFd {
    pub fn new(count: u64, flags: EventFdFlags, id: u32) -> Self {
        EventFd { count, flags, id }
    }
}
pub struct EventFdInode {
    eventfd: Mutex<EventFd>,
    wait_queue: Mutex<VecDeque<usize>>,
}

impl Debug for EventFdInode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventFdInode")
            .field("eventfd", &self.eventfd)
            .finish()
    }
}

impl EventFdInode {
    pub fn new(eventfd: EventFd) -> Self {
        EventFdInode {
            eventfd: Mutex::new(eventfd),
            wait_queue: Mutex::new(VecDeque::new()),
        }
    }
}

impl File for EventFdInode {
    fn read(&self, mut buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        if buf.len() < 8 {
            return Err(AlienError::EINVAL);
        }
        let mut val = loop {
            let val = self.eventfd.lock().count;
            if val != 0 {
                break val;
            }
            if self
                .eventfd
                .lock()
                .flags
                .contains(EventFdFlags::EFD_NONBLOCK)
            {
                return Err(AlienError::EAGAIN);
            }
            let tid = basic::current_tid()?.unwrap();
            self.wait_queue.lock().push_back(tid);
            basic::wait_now()?; // yield current task
        };
        let mut eventfd = self.eventfd.lock();
        if eventfd.flags.contains(EventFdFlags::EFD_SEMAPHORE) {
            eventfd.count -= 1;
            val = 1;
        } else {
            eventfd.count = 0;
        }
        let val_bytes = val.to_ne_bytes();
        buf.as_mut_slice()[..8].copy_from_slice(&val_bytes);
        Ok((buf, 8))
    }
    fn write(&self, buf: &DVec<u8>) -> AlienResult<usize> {
        if buf.len() < 8 {
            return Err(AlienError::EINVAL);
        }
        let val = u64::from_ne_bytes(buf.as_slice()[..8].try_into().unwrap());
        if val == u64::MAX {
            return Err(AlienError::EINVAL);
        }
        loop {
            let eventfd = self.eventfd.lock();
            if u64::MAX - eventfd.count > val {
                break;
            }
            // block until a read() is performed  on the
            // file descriptor, or fails with the error EAGAIN if the
            // file descriptor has been made nonblocking.
            if eventfd.flags.contains(EventFdFlags::EFD_NONBLOCK) {
                return Err(AlienError::EAGAIN);
            }
            drop(eventfd);
            // self.wait_queue.sleep();
            let tid = basic::current_tid()?.unwrap();
            self.wait_queue.lock().push_back(tid);
            basic::wait_now()?; // yield current task
        }
        let mut eventfd = self.eventfd.lock();
        eventfd.count += val;
        while let Some(tid) = self.wait_queue.lock().pop_front() {
            basic::wake_up_wait_task(tid)?;
        }
        Ok(8)
    }
    fn read_at(&self, _offset: u64, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        self.read(buf)
    }
    fn write_at(&self, _offset: u64, buf: &DVec<u8>) -> AlienResult<usize> {
        self.write(buf)
    }

    fn seek(&self, _pos: SeekFrom) -> AlienResult<u64> {
        Err(AlienError::ENOSYS)
    }

    fn get_attr(&self) -> AlienResult<VfsFileStat> {
        Err(AlienError::ENOSYS)
    }

    fn dentry(&self) -> Arc<dyn VfsDentry> {
        panic!("EventFdInode::dentry() is not implemented")
    }

    fn inode(&self) -> Arc<dyn VfsInode> {
        panic!("EventFdInode::inode() is not implemented")
    }

    fn is_readable(&self) -> bool {
        true
    }

    fn is_writable(&self) -> bool {
        true
    }

    fn is_append(&self) -> bool {
        true
    }

    fn poll(&self, event: PollEvents) -> AlienResult<PollEvents> {
        let mut events = PollEvents::empty();
        if self.eventfd.lock().count != 0 && event.contains(PollEvents::EPOLLIN) {
            events |= PollEvents::EPOLLIN;
        }
        if self.eventfd.lock().count != u64::MAX && event.contains(PollEvents::EPOLLOUT) {
            events |= PollEvents::EPOLLOUT
        }
        return Ok(events);
    }
}

pub fn eventfd(init_val: u32, flags: u32) -> AlienResult<Arc<dyn File>> {
    let flags = EventFdFlags::from_bits_truncate(flags);
    // println_color!(32, "eventfd: init_val: {}, flags: {:?}", init_val, flags);
    let id = EVENTFD_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let eventfd = EventFd::new(init_val as u64, flags, id);
    let inode = Arc::new(EventFdInode::new(eventfd));
    Ok(inode)
}
