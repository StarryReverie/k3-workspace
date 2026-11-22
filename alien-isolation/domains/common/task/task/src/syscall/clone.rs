use basic::{
    constants::{signal::SignalNumber, task::CloneFlags},
    *,
};

use crate::{
    processor::{add_task, current_task},
    task::CloneArgs,
};
pub fn do_clone(
    flags: usize,
    stack: usize,
    ptid: usize,
    tls: usize,
    ctid: usize,
) -> AlienResult<isize> {
    trace!(
        "clone: flags: {:#x}, stack: {:#x}, ptid: {:#x}, tls: {:#x}, ctid: {:#x}",
        flags,
        stack,
        ptid,
        tls,
        ctid
    );
    let clone_flag = CloneFlags::from_bits_truncate(flags as u32);
    // check whether flag include signal
    let sig = flags & 0xff;
    let sig = SignalNumber::try_from(sig as u8).map_err(|_| AlienError::EINVAL)?;
    let task = current_task().unwrap();
    let clone_args = CloneArgs {
        flags: clone_flag,
        stack,
        ptid,
        tls,
        ctid,
        sig,
    };
    let new_task = task.do_clone(clone_args).ok_or(AlienError::EAGAIN)?;
    // update return value
    let trap_frame = new_task.trap_frame();
    trap_frame.update_result(0);
    let tid = new_task.tid.raw();
    // println_color!(33, "clone: new task tid: {}", tid);
    add_task(new_task);
    Ok(tid as isize)
}
