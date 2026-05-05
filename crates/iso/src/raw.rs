use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const SECTOR_SIZE: usize = 2352;
pub const USER_DATA_OFFSET: usize = 24;
pub const USER_DATA_SIZE: usize = 2048;

pub struct RawDisc {
    file: File,
    sector_count: u64,
}

impl RawDisc {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file,
            sector_count: len / SECTOR_SIZE as u64,
        })
    }

    pub fn sector_count(&self) -> u64 {
        self.sector_count
    }

    pub fn read_sector(&mut self, lba: u32) -> io::Result<[u8; USER_DATA_SIZE]> {
        let mut sector = [0u8; SECTOR_SIZE];
        self.file
            .seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        self.file.read_exact(&mut sector)?;
        let mut out = [0u8; USER_DATA_SIZE];
        out.copy_from_slice(&sector[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE]);
        Ok(out)
    }

    pub fn read_user_data(&mut self, lba: u32, count: u32, out: &mut Vec<u8>) -> io::Result<()> {
        out.clear();
        out.reserve(count as usize * USER_DATA_SIZE);
        let mut sector = [0u8; SECTOR_SIZE];
        self.file
            .seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        for _ in 0..count {
            self.file.read_exact(&mut sector)?;
            out.extend_from_slice(&sector[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE]);
        }
        Ok(())
    }
}
