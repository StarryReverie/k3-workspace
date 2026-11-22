use alloc::sync::Arc;

use basic::{
    config::CLOCK_FREQ,
    constants::time::{ClockId, TimeSpec, TimeVal},
    time::{read_timer, TimeNow},
    AlienError, AlienResult,
};
use interface::TaskDomain;
use pod::Pod;

pub fn sys_clock_gettime(
    task_domain: &Arc<dyn TaskDomain>,
    clk_id: usize,
    tp: usize,
) -> AlienResult<isize> {
    let id = ClockId::try_from(clk_id).map_err(|_| AlienError::EINVAL)?;
    match id {
        ClockId::Monotonic | ClockId::Realtime | ClockId::ProcessCputimeId => {
            let time = read_timer();
            let time = TimeSpec {
                tv_sec: time / CLOCK_FREQ,
                tv_nsec: (time % CLOCK_FREQ) * 1000_000_000 / CLOCK_FREQ,
            };
            task_domain.copy_to_user(tp, time.as_bytes())?;
            Ok(0)
        }
        _ => {
            panic!("clock_get_time: clock_id {:?} not supported", id);
        }
    }
}

pub fn sys_get_time_of_day(task_domain: &Arc<dyn TaskDomain>, tv: usize) -> AlienResult<isize> {
    let time = TimeVal::now();
    task_domain.write_val_to_user(tv, &time)?;
    Ok(0)
}
