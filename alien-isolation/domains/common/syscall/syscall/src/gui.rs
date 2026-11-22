use alloc::{sync::Arc, vec::Vec};

use basic::{config::FRAME_SIZE, AlienError, AlienResult};
use interface::{BufInputDomain, GpuDomain, TaskDomain};
use log::info;

pub fn sys_framebuffer_flush(gpu: Option<&Arc<dyn GpuDomain>>) -> AlienResult<isize> {
    info!("<sys_framebuffer_flush>");
    let gpu = gpu.ok_or(AlienError::EINVAL)?;
    gpu.flush()?;
    Ok(0)
}

pub fn sys_framebuffer(
    task_domain: &Arc<dyn TaskDomain>,
    gpu: Option<&Arc<dyn GpuDomain>>,
) -> AlienResult<isize> {
    let gpu = gpu.ok_or(AlienError::EINVAL)?;
    let gpu_phy_buf = gpu.buffer_range()?;
    assert_eq!(gpu_phy_buf.start % FRAME_SIZE, 0);
    let device_mmap = task_domain.do_mmap_device(gpu_phy_buf)?;
    info!("<sys_framebuffer> device_mmap: {:#x}", device_mmap);
    Ok(device_mmap)
}

pub fn sys_event_get(
    task_domain: &Arc<dyn TaskDomain>,
    input: &[Arc<dyn BufInputDomain>],
    event_buf: usize,
    len: usize,
) -> AlienResult<isize> {
    info!("<sys_event_get> event_buf: {:#x}, len: {}", event_buf, len);
    let mut events = Vec::with_capacity(len * 8);
    let mut count = 0;
    'outer: for input in input {
        while input.have_event()? {
            if count >= len {
                break 'outer;
            }
            let event = input.event_block()?;
            info!("event: {:#x}", event);
            count += 1;
            let event = event.to_le_bytes();
            events.extend_from_slice(&event);
        }
    }
    // println!("<sys_event_get> get {} events", count);
    assert!(events.len() <= len * 8);
    task_domain.copy_to_user(event_buf, &events.as_slice()[..count * 8])?;
    Ok(count as isize)
}
