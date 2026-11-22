use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::cmp::min;

use basic::{sync::Mutex, AlienError, AlienResult};

/// 用于记录一个进程等待一个 futex 的相关信息
#[allow(unused)]
pub struct FutexWaiter {
    /// 进程的控制块
    task: Option<usize>,
    /// 进程等待 futex 的等待时间
    wait_time: Option<usize>,
    /// 超时事件的标志位，标识该进程对于 futex 等待是否超时
    timeout_flag: Arc<Mutex<bool>>,
    bitset: u32,
}

impl FutexWaiter {
    /// 创建一个新的 `FutexWaiter` 保存等待在某 futex 上的一个进程 有关等待的相关信息
    pub fn new(
        task_tid: usize,
        wait_time: Option<usize>,
        timeout_flag: Arc<Mutex<bool>>,
        bitset: u32,
    ) -> Self {
        Self {
            task: Some(task_tid),
            wait_time,
            timeout_flag,
            bitset,
        }
    }

    /// Return the tid of the task
    pub fn wake(&mut self) -> usize {
        self.task.take().unwrap()
    }
}

/// 用于管理 futex 等待队列的数据结构
///
/// 包含一个 futex id -> futexWait Vec 的 map
pub struct FutexWaitManager {
    map: BTreeMap<usize, Vec<FutexWaiter>>,
}

impl FutexWaitManager {
    /// 创建一个新的 futex 管理器，保存 futex 和在其上等待队列的映射关系
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
    /// 在某等待队列中加入等待进程
    pub fn add_waiter(&mut self, futex: usize, waiter: FutexWaiter) {
        self.map.entry(futex).or_insert(Vec::new()).push(waiter);
    }
    /// 唤醒 futex 上的至多 num 个等待的进程
    pub fn wake(&mut self, futex: usize, num: usize, bitset: u32) -> AlienResult<usize> {
        if let Some(waiters) = self.map.get_mut(&futex) {
            let min_index = min(num, waiters.len());
            let mut count = 0;
            for i in 0..min_index {
                if (waiters[i].bitset & bitset) != 0 {
                    let tid = waiters[i].wake();
                    basic::wake_up_wait_task(tid)?;
                    waiters.remove(i);
                    count += 1;
                }
            }
            Ok(count)
        } else {
            // println_color!(31,"futex {} not found", futex);
            Err(AlienError::EINVAL)
        }
    }

    /// 将原来等待在 old_futex 上至多 num 个进程转移到 requeue_futex 上等待，返回转移的进程数
    pub fn requeue(
        &mut self,
        requeue_futex: usize,
        num: usize,
        old_futex: usize,
    ) -> AlienResult<usize> {
        if num == 0 {
            return Ok(0);
        }
        // move waiters
        let mut waiters = self.map.remove(&old_futex).unwrap();
        // create new waiters
        let new_waiters = self.map.entry(requeue_futex).or_insert(Vec::new());
        let min_index = min(num, waiters.len());
        error!("requeue {} waiters", min_index);
        for _ in 0..min_index {
            let waiter = waiters.pop().unwrap();
            new_waiters.push(waiter);
        }
        // insert old waiters
        if !waiters.is_empty() {
            self.map.insert(old_futex, waiters);
        }
        Ok(min_index)
    }
}
