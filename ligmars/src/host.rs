//! Structs and functions for connecting to an LGMP shared memory connction as a host.
//! Requres the [host] feature enabled to use.
use std::mem::MaybeUninit;

use crate::{
    error::{LGMPResult, Status},
    shm_file::ShmFileHandle,
};

/// Handle to a SHM communication file as the host.
pub struct LGMPHost {
    source: Option<Box<dyn ShmFileHandle>>,
    inner: liblgmp_sys::PLGMPHost,
    allocations: Vec<LGMPMemoryAllocation>,
}

impl LGMPHost {
    /// Initialises a handle to the host side of a LGMP connection
    /// given a handle to a memory mapped SHM file.
    /// The SHM file and memory mapped region must be large enough to
    /// handle all communications, as it cannot be resized during use.
    pub fn init(mut file: Box<dyn ShmFileHandle>, udata: Vec<u8>) -> LGMPResult<LGMPHost> {
        let ptr = file.get_mut_ptr() as *mut std::ffi::c_void;
        let size: usize = file.get_size();

        let mut host: LGMPHost = unsafe { Self::init_from_ptr(ptr, size, udata)? };

        host.source = Some(file);
        Ok(host)
    }

    /// Initialises a handle to the host side of a LGMP connection
    /// from a raw pointer. This pointer will usually be into a memory-mapped
    /// file to be used for communication.
    /// The caller must handle cleanup of the memory location.
    ///
    /// # Safety
    /// The pointer provided for mem must point into a valid
    /// writeable memory location which can be used for communication.
    /// There must also be at least `size` bytes of usable memory available.
    pub unsafe fn init_from_ptr(
        mem: *mut std::ffi::c_void,
        size: usize,
        mut udata: Vec<u8>,
    ) -> LGMPResult<LGMPHost> {
        let mut res_ptr: MaybeUninit<liblgmp_sys::PLGMPHost> = MaybeUninit::zeroed();
        let size = u32::try_from(size)?;
        let udata_ptr = udata.as_mut_ptr();
        let udata_size = u32::try_from(udata.len())?;
        let res: u32 = unsafe {
            liblgmp_sys::lgmpHostInit(mem, size, res_ptr.as_mut_ptr(), udata_size, udata_ptr)
        };
        let allocations: Vec<LGMPMemoryAllocation> = Vec::new();

        Status::from(res)
            .ok_and_init_if_success(res_ptr)
            .map(|inner| LGMPHost {
                inner,
                source: None,
                allocations,
            })
    }

    /// Runs the garbage collector on message queues.
    ///
    /// When run, this function reads messages from each of the queues and removes any that have
    /// been read by all listeners.
    /// It also checks for any messages that have passed their timeout even if they have not been
    /// read by all listeners. In this case, it sets a timeout at which point any listeners can
    /// be considered disconnected and therefore garbage collected. It will also remove any such
    /// messages.
    ///
    /// This function should be called regularly to ensure that messages are properly cleaned up.
    pub fn process(&mut self) -> LGMPResult<()> {
        let host = self.inner;

        let res = unsafe { liblgmp_sys::lgmpHostProcess(host) };

        Status::from(res).ok_if_success(())
    }

    /// Creates a new queue from the provided options.
    pub fn queue_new(&mut self, config: LGMPQueueConfig) -> LGMPResult<LGMPHostQueue> {
        let host = self.inner;
        let mut res_queue: MaybeUninit<liblgmp_sys::PLGMPHostQueue> = MaybeUninit::zeroed();

        let res =
            unsafe { liblgmp_sys::lgmpHostQueueNew(host, config.into(), res_queue.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(res_queue)
            .map(|inner| LGMPHostQueue { inner })
    }

    /// Returns the amount of memory in bytes which is still available to be
    /// allocated within the shared memory region
    pub fn mem_available(&self) -> usize {
        let host = self.inner;

        unsafe { liblgmp_sys::lgmpHostMemAvail(host) }
    }

    /// Allocates a block of memory of size `size` bytes within the shared
    /// memory region.
    ///
    /// This is equivalent to calling
    /// ```
    /// self.mem_alloc_aligned(size, 4)
    ///
    /// ```
    ///
    /// Note that allocations are permanent; whilst the struct pointing to memory
    /// allocations will be freed upon being dropped, the memory within the shared
    /// memory location will never be recovered until the host is closed.
    pub fn mem_alloc(&mut self, size: u32) -> LGMPResult<&LGMPMemoryAllocation> {
        self.mem_alloc_aligned(size, 4)
    }

    /// Allocates a block of memory of size `size` bytes within the shared
    /// memory region aligned to the specified bit alignment.
    ///
    /// This will consume up to `size + alignment` bytes from the available pool.
    ///
    /// Note that allocations are permanent; whilst the struct pointing to memory
    /// allocations will be freed upon being dropped, the memory within the shared
    /// memory location will never be recovered until the host is closed.
    pub fn mem_alloc_aligned(
        &mut self,
        size: u32,
        alignment: u32,
    ) -> LGMPResult<&LGMPMemoryAllocation> {
        let host = self.inner;
        let mut allocation: MaybeUninit<liblgmp_sys::PLGMPMemory> = MaybeUninit::zeroed();

        let res = unsafe {
            liblgmp_sys::lgmpHostMemAllocAligned(host, size, alignment, allocation.as_mut_ptr())
        };

        Status::from(res)
            .ok_and_init_if_success(allocation)
            .map(|allocation| {
                let allocation_struct = LGMPMemoryAllocation { inner: allocation };
                self.allocations.push(allocation_struct);
                self.allocations
                    .last()
                    .expect("Somehow allocations vector was empty after pushing something to it")
            })
    }
}

impl Drop for LGMPHost {
    fn drop(&mut self) {
        //Call free in C library
        unsafe { liblgmp_sys::lgmpHostFree(&mut self.inner) }
    }
}

/// Configuration struct to be used when creating a new queue
pub struct LGMPQueueConfig {
    /// Application defined queue ID
    pub queue_id: u32,
    /// Number of messages in the queue
    pub num_messages: u32,
    /// Length of time in milliseconds to wait before removing a subscriber
    pub sub_timeout: u32,
}

impl From<LGMPQueueConfig> for liblgmp_sys::LGMPQueueConfig {
    fn from(val: LGMPQueueConfig) -> Self {
        liblgmp_sys::LGMPQueueConfig {
            queueID: val.queue_id,
            numMessages: val.num_messages,
            subTimeout: val.sub_timeout,
        }
    }
}

/// Handle to an LGMP queue from the host side.
pub struct LGMPHostQueue {
    inner: liblgmp_sys::PLGMPHostQueue,
}

impl LGMPHostQueue {
    /// Returns true iff there are one or more subscribers listening
    /// on the queue
    pub fn has_subs(&self) -> bool {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueueHasSubs(queue) }
    }

    /// Returns the number of new listeners that have subscribed to this channel
    /// since the last time this function was called.
    pub fn new_subs(&self) -> u32 {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueueNewSubs(queue) }
    }

    /// Returns the number of pending messages that currently exist in this
    /// channel
    pub fn pending(&self) -> u32 {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueuePending(queue) }
    }

    /// Posts a new message to the channel containing both a reference to an allocated
    /// block of shared memory, and a 32-bit integer of user-specified data.
    ///
    /// Whilst this 32-bit udata field can contain any value, it is probably most useful
    /// for use as eg. a sequential message ID.
    pub fn post_shared_mem(
        &mut self,
        udata: u32,
        payload: &LGMPMemoryAllocation,
    ) -> LGMPResult<()> {
        let queue = self.inner;
        let payload = payload.inner;

        let res = unsafe { liblgmp_sys::lgmpHostQueuePost(queue, udata, payload) };

        Status::from(res).ok_if_success(())
    }

    /// Reads a message from the queue, sent by a client, returning LGMPErrQueueEmpty if
    /// no messages exist.
    pub fn read_data(&mut self) -> LGMPResult<Vec<u8>> {
        let queue = self.inner;
        let mut tgt: Vec<u8> = Vec::with_capacity(
            liblgmp_sys::LGMP_MSGS_SIZE
                .try_into()
                .expect("Message size in underlying library exceeded maxint?!?"),
        );
        let mut tgt_len: usize = 0;

        let res = unsafe {
            liblgmp_sys::lgmpHostReadData(
                queue,
                tgt.as_mut_ptr() as *mut std::ffi::c_void,
                &mut tgt_len,
            )
        };

        Status::from(res)
            .ok_if_success((tgt, tgt_len))
            .map(|(mut data, len)| {
                unsafe { data.set_len(len) }
                data
            })
    }

    /// Increments the client message read serial to indicate that a client message
    /// has been read from the channel.
    pub fn ack_data(&mut self) -> LGMPResult<()> {
        let queue = self.inner;

        let res = unsafe { liblgmp_sys::lgmpHostAckData(queue) };

        Status::from(res).ok_if_success(())
    }

    /// Retrieves a list of IDs of clients which are subscribed to the current
    /// channel.
    pub fn get_client_ids(&self) -> LGMPResult<Vec<u32>> {
        let queue = self.inner;
        let mut client_ids_vec: Vec<u32> = Vec::with_capacity(32);
        let mut count: u32 = 0;

        let res = unsafe {
            liblgmp_sys::lgmpHostGetClientIDs(queue, client_ids_vec.as_mut_ptr(), &mut count)
        };

        Status::from(res)
            .ok_if_success((client_ids_vec, count))
            .and_then(|(client_ids_vec, count)| Ok((client_ids_vec, usize::try_from(count)?)))
            .map(|(mut client_ids_vec, count)| {
                unsafe { client_ids_vec.set_len(count) }
                client_ids_vec
            })
    }
}

/// A handle to an allocation of shared memory.
///
/// Note that whilst the handle itself will be correctly freed upon drop,
/// LGMP memory allocations are permanent, and as such any allocated memory
/// will never be freed until the host program restarts.
pub struct LGMPMemoryAllocation {
    inner: liblgmp_sys::PLGMPMemory,
}

impl LGMPMemoryAllocation {
    /// Returns a raw mutable pointer to a chunk of allocated memory.
    pub fn mem_ptr(&mut self) -> *mut std::ffi::c_void {
        let mem = self.inner;
        unsafe { liblgmp_sys::lgmpHostMemPtr(mem) }
    }
}

impl Drop for LGMPMemoryAllocation {
    fn drop(&mut self) {
        unsafe { liblgmp_sys::lgmpHostMemFree(&mut self.inner) }
    }
}
