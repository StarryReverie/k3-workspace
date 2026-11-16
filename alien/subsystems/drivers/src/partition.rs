//! MBR/GPT 分区解析与偏移包装。
//!
//! 阶段 9 只需要找到第一个 FAT 分区。解析过程只读，不修改分区表。

use alloc::boxed::Box;

use constants::AlienResult;
use device_interface::LowBlockDevice;

const BLOCK_SIZE: usize = 512;

/// 一个分区的位置与大小。
#[derive(Clone, Copy, Debug)]
pub struct Partition {
    pub lba_offset: u64,
    pub blocks: usize,
}

/// 把分区偏移包装成独立块设备。
pub struct PartitionBlockDevice {
    inner: Box<dyn LowBlockDevice>,
    lba_offset: u64,
    blocks: usize,
}

impl PartitionBlockDevice {
    pub fn new(inner: Box<dyn LowBlockDevice>, partition: Partition) -> Self {
        Self {
            inner,
            lba_offset: partition.lba_offset,
            blocks: partition.blocks,
        }
    }
}

impl LowBlockDevice for PartitionBlockDevice {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) -> AlienResult<()> {
        let lba = self.lba_offset + block_id as u64;
        self.inner.read_block(lba as usize, buf)
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) -> AlienResult<()> {
        let lba = self.lba_offset + block_id as u64;
        self.inner.write_block(lba as usize, buf)
    }

    fn capacity(&self) -> usize {
        self.blocks
    }

    fn read_block_async(&self, block_id: usize, buf: &mut [u8]) -> AlienResult<()> {
        self.read_block(block_id, buf)
    }

    fn write_block_async(&self, block_id: usize, buf: &[u8]) -> AlienResult<()> {
        self.write_block(block_id, buf)
    }

    fn handle_irq(&self) {
        self.inner.handle_irq();
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// 查找第一个可挂载的 FAT 分区。
///
/// 先尝试 GPT，再尝试 MBR；若 0 号扇区本身是 FAT 引导扇区，则按整盘处理。
pub fn find_fat_partition(device: &dyn LowBlockDevice) -> Option<Partition> {
    let mut mbr = [0u8; BLOCK_SIZE];
    if device.read_block(0, &mut mbr).is_err() {
        return None;
    }

    if mbr[510] == 0x55 && mbr[511] == 0xaa {
        if let Some(partition) = find_gpt(device, &mbr) {
            return Some(partition);
        }
        if let Some(partition) = find_mbr(device, &mbr) {
            return Some(partition);
        }
    }

    if is_fat_boot(device, 0) {
        return Some(Partition {
            lba_offset: 0,
            blocks: device.capacity(),
        });
    }
    None
}

fn find_mbr(device: &dyn LowBlockDevice, mbr: &[u8; BLOCK_SIZE]) -> Option<Partition> {
    for index in 0..4 {
        let entry = 0x1be + index * 16;
        let part_type = mbr[entry + 4];
        if part_type == 0 {
            continue;
        }
        let lba = u32::from_le_bytes(mbr[entry + 8..entry + 12].try_into().ok()?) as u64;
        let sectors = u32::from_le_bytes(mbr[entry + 12..entry + 16].try_into().ok()?) as usize;
        if sectors == 0 {
            continue;
        }
        if is_fat_boot(device, lba) {
            return Some(Partition {
                lba_offset: lba,
                blocks: sectors,
            });
        }
    }
    None
}

fn find_gpt(device: &dyn LowBlockDevice, mbr: &[u8; BLOCK_SIZE]) -> Option<Partition> {
    if !has_protective_mbr(mbr) {
        return None;
    }

    let mut header = [0u8; BLOCK_SIZE];
    if device.read_block(1, &mut header).is_err() {
        return None;
    }
    if &header[0..8] != b"EFI PART" {
        return None;
    }

    let entry_lba = u64::from_le_bytes(header[72..80].try_into().ok()?);
    let entry_count = u32::from_le_bytes(header[80..84].try_into().ok()?) as usize;
    let entry_size = u32::from_le_bytes(header[84..88].try_into().ok()?) as usize;
    if entry_size < 128 || entry_size > BLOCK_SIZE {
        return None;
    }

    let per_block = BLOCK_SIZE / entry_size;
    if per_block == 0 {
        return None;
    }
    let blocks = (entry_count + per_block - 1) / per_block;
    for block_index in 0..blocks.min(32) {
        let mut block = [0u8; BLOCK_SIZE];
        if device
            .read_block(entry_lba as usize + block_index, &mut block)
            .is_err()
        {
            return None;
        }
        for slot in 0..per_block {
            let offset = slot * entry_size;
            if offset + 128 > BLOCK_SIZE {
                break;
            }
            let entry = &block[offset..offset + 128];
            if entry[0..16].iter().all(|&byte| byte == 0) {
                continue;
            }
            let first = u64::from_le_bytes(entry[32..40].try_into().ok()?);
            let last = u64::from_le_bytes(entry[40..48].try_into().ok()?);
            if last < first {
                continue;
            }
            if is_fat_boot(device, first) {
                return Some(Partition {
                    lba_offset: first,
                    blocks: (last - first + 1) as usize,
                });
            }
        }
    }
    None
}

fn has_protective_mbr(mbr: &[u8; BLOCK_SIZE]) -> bool {
    (0..4).any(|index| mbr[0x1be + index * 16 + 4] == 0xee)
}

fn is_fat_boot(device: &dyn LowBlockDevice, lba: u64) -> bool {
    let mut buf = [0u8; BLOCK_SIZE];
    if device.read_block(lba as usize, &mut buf).is_err() {
        return false;
    }
    (buf[0] == 0xeb || buf[0] == 0xe9) && buf[510] == 0x55 && buf[511] == 0xaa
}
