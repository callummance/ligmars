//!Trait used for access to a memory-mapped SHM file, as well as implementations.

/// Trait for a handle to shared memory of some kind, through which
/// interprocess communication can take place.
pub trait ShmFileHandle {
    fn get_mut_ptr(&mut self) -> *mut std::ffi::c_void;
    fn get_size(&self) -> usize;
}

cfg_if::cfg_if! {
    if #[cfg(feature = "internal_shm_impl")] {
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

        impl ShmFileHandle for ShmFile {
            fn get_mut_ptr(&mut self) -> *mut std::ffi::c_void {
                self.get_ptr() as *mut std::ffi::c_void
            }

            fn get_size(&self) -> usize {
                usize::try_from(self.file_size).expect("File size exceeded maxint")
            }
        }
    }

}

pub use shared_memory;

impl ShmFileHandle for shared_memory::Shmem {
    fn get_mut_ptr(&mut self) -> *mut std::ffi::c_void {
        self.as_ptr() as *mut std::ffi::c_void
    }

    fn get_size(&self) -> usize {
        self.len()
    }
}
