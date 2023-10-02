use std::fs::OpenOptions;

use memmap2::MmapOptions;

use crate::error;

/// ShmFile holds a pointer to a memory mapped SHM file along with associated metadata
pub struct ShmFile {
    pub file_path: std::path::PathBuf,
    pub file_size: u64,
    mapped: memmap2::MmapRaw,
}

impl ShmFile {
    /// Opens an existing SHM file found at the provided path, and maps it into memory.
    pub fn open(file_path: std::path::PathBuf) -> error::LGMPResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(&file_path)?;

        let file_size = file.metadata()?.len();

        let mapped = MmapOptions::new()
            .len(file_size.try_into().expect("File length exceeed usize"))
            .map_raw(&file)?;

        Ok(ShmFile {
            file_path,
            file_size,
            mapped,
        })
    }

    pub fn get_ptr(&self) -> *mut () {
        self.mapped.as_mut_ptr() as *mut ()
    }
}
