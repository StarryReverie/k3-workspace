use alloc::vec::Vec;

use basic::{
    constants::signal::{SignalStack, *},
    AlienResult,
};
use memory_addr::VirtAddr;
use pod::Pod;

use crate::processor::current_task;

pub fn do_sigaction(sig: u8, action: usize, old_action: usize) -> AlienResult<isize> {
    let action = action as *const SigAction;
    let old_action = old_action as *mut SigAction;
    let task = current_task().unwrap();

    let signum = SignalNumber::try_from(sig).unwrap();
    let signal_handler = task.signal_handlers.clone();
    let mut signal_handler = signal_handler.lock();
    if !old_action.is_null() {
        let mut tmp = SigAction::empty();
        signal_handler.get_action(sig as _, &mut tmp);
        task.write_bytes_to_user(VirtAddr::from(old_action as usize), tmp.as_bytes())?;
    }
    if !action.is_null() {
        let mut tmp_action = SigAction::empty();
        task.read_bytes_from_user(VirtAddr::from(action as _), tmp_action.as_bytes_mut())?;
        warn!("sig {:?} action is {:?}", signum, tmp_action);
        signal_handler.set_action(sig as _, &tmp_action);
    }
    Ok(0)
}

pub fn do_sigprocmask(how: usize, set: usize, oldset: usize) -> AlienResult<isize> {
    trace!(
        "sigprocmask: how: {:x}, set: {:#x}, oldset: {:x}",
        how,
        set,
        oldset
    );
    let task = current_task().unwrap();
    let mut signal_receivers = task.signal_receivers.lock();
    if oldset != 0 {
        let val = signal_receivers.mask.bits();
        task.write_val_to_user(VirtAddr::from(oldset), &val)?;
    }
    let how = SigProcMaskHow::try_from(how).unwrap();
    // println_color!(32,"sigprocmask: how: {:?}, set: {:#x}", how, set);
    if set != 0 {
        let set = task.read_val_from_user::<usize>(VirtAddr::from(set))?;
        match how {
            SigProcMaskHow::SigBlock => {
                signal_receivers.mask += SimpleBitSet::from(set);
            }
            SigProcMaskHow::SigUnblock => {
                signal_receivers.mask -= SimpleBitSet::from(set);
            }
            SigProcMaskHow::SigSetMask => {
                signal_receivers.mask = SimpleBitSet::from(set);
            }
        }
    }
    let mask: Vec<SignalNumber> = signal_receivers.mask.into();
    trace!("after sigprocmask: {:?}", mask);
    Ok(0)
}

/// See https://man7.org/linux/man-pages/man2/sigaltstack.2.html
pub fn do_signal_stack(uss: usize, uoss: usize) -> AlienResult<isize> {
    // println_color!(32, "sigaltstack: uss: {:x}, uoss: {:x}", uss, uoss);
    let task = current_task().unwrap();
    if uoss != 0 {
        let old_ss_stack = task.inner().ss_stack;
        // println_color!(32, "get old sigaltstack: {:x?}", old_ss_stack);
        task.write_val_to_user(VirtAddr::from(uoss), &old_ss_stack)?;
    }
    if uss != 0 {
        let ss_stack = task.read_val_from_user::<SignalStack>(VirtAddr::from(uss))?;
        // println_color!(32, "set sigaltstack: {:x?}", ss_stack);
        task.inner().ss_stack = ss_stack;
    }
    Ok(0)
}
