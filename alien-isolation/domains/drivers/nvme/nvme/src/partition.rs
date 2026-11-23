//! Read-only MBR/GPT parsing for the NVMe domain.
//!
//! The block stack expects one block device, so the first FAT partition is
//! selected and exposed with its own offset. A whole-disk FAT image (no
//! partition table) is also accepted.

const BLOCK_SIZE: usize = 512;

/// Selected partition.
#[derive(Clone, Copy, Debug)]
pub struct Partition {
    pub lba_offset: u64,
    pub blocks: u64,
}

type Reader<'a> = dyn FnMut(u64, &mut [u8]) -> bool + 'a;

/// Find the first mountable FAT partition.
pub fn find_fat_partition(read: &mut Reader<'_>, capacity: u64) -> Option<Partition> {
    let mut mbr = [0u8; BLOCK_SIZE];
    if !read(0, &mut mbr) {
        return None;
    }

    if mbr[510] == 0x55 && mbr[511] == 0xaa {
        if let Some(partition) = find_gpt(read, &mbr) {
            return Some(partition);
        }
        if let Some(partition) = find_mbr(read, &mbr) {
            return Some(partition);
        }
    }

    if is_fat_boot(read, 0) {
        return Some(Partition {
            lba_offset: 0,
            blocks: capacity,
        });
    }
    None
}

fn find_mbr(read: &mut Reader<'_>, mbr: &[u8; BLOCK_SIZE]) -> Option<Partition> {
    for index in 0..4 {
        let entry = 0x1be + index * 16;
        let part_type = mbr[entry + 4];
        if part_type == 0 {
            continue;
        }
        let lba = u32::from_le_bytes(mbr[entry + 8..entry + 12].try_into().ok()?) as u64;
        let sectors = u32::from_le_bytes(mbr[entry + 12..entry + 16].try_into().ok()?) as u64;
        if sectors == 0 {
            continue;
        }
        if is_fat_boot(read, lba) {
            return Some(Partition {
                lba_offset: lba,
                blocks: sectors,
            });
        }
    }
    None
}

fn find_gpt(read: &mut Reader<'_>, mbr: &[u8; BLOCK_SIZE]) -> Option<Partition> {
    if !(0..4).any(|index| mbr[0x1be + index * 16 + 4] == 0xee) {
        return None;
    }

    let mut header = [0u8; BLOCK_SIZE];
    if !read(1, &mut header) {
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
        if !read(entry_lba + block_index as u64, &mut block) {
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
            if is_fat_boot(read, first) {
                return Some(Partition {
                    lba_offset: first,
                    blocks: last - first + 1,
                });
            }
        }
    }
    None
}

fn is_fat_boot(read: &mut Reader<'_>, lba: u64) -> bool {
    let mut buf = [0u8; BLOCK_SIZE];
    if !read(lba, &mut buf) {
        return false;
    }
    (buf[0] == 0xeb || buf[0] == 0xe9) && buf[510] == 0x55 && buf[511] == 0xaa
}
