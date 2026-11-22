use alloc::{collections::BTreeMap, sync::Arc};

use basic::{sync::Mutex, wake_up_wait_task};

use crate::task::Task;

pub fn current_task() -> Option<Arc<Task>> {
    let tid = basic::current_tid().unwrap()?;
    let task = GLOBAL_TASK_MANAGER.lock().get(&tid).map(Arc::clone);
    task
}

static GLOBAL_TASK_MANAGER: Mutex<BTreeMap<usize, Arc<Task>>> = Mutex::new(BTreeMap::new());

pub fn add_task(task: Arc<Task>) {
    let tid = task.tid();
    GLOBAL_TASK_MANAGER.lock().insert(tid, task);
    wake_up_wait_task(tid).unwrap();
}

pub fn remove_task(tid: usize) {
    GLOBAL_TASK_MANAGER.lock().remove(&tid);
}

pub fn find_task(tid: usize) -> Option<Arc<Task>> {
    GLOBAL_TASK_MANAGER.lock().get(&tid).map(Arc::clone)
}
