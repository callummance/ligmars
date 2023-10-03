use std::mem::MaybeUninit;

use crate::{
    error::{LGMPResult, Status},
    shm_file::ShmFileHandle,
};

pub struct LGMPQueueConfig {
    queue_id: u32,
    num_messages: u32,
    sub_timeout: u32,
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

pub struct LGMPHost {
    source: Option<Box<dyn ShmFileHandle>>,
    inner: liblgmp_sys::PLGMPHost,
    allocations: Vec<LGMPMemoryAllocation>,
}

impl LGMPHost {
    pub fn init(mut file: Box<dyn ShmFileHandle>, udata: Vec<u8>) -> LGMPResult<LGMPHost> {
        let ptr = file.get_mut_ptr() as *mut std::ffi::c_void;
        let size: usize = file.get_size();

        let mut host: LGMPHost = unsafe { Self::init_from_ptr(ptr, size, udata)? };

        host.source = Some(file);
        Ok(host)
    }

    unsafe fn init_from_ptr(
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

    pub fn process(&mut self) -> LGMPResult<()> {
        let host = self.inner;

        let res = unsafe { liblgmp_sys::lgmpHostProcess(host) };

        Status::from(res).ok_if_success(())
    }

    pub fn queue_new(&mut self, config: LGMPQueueConfig) -> LGMPResult<LGMPHostQueue> {
        let host = self.inner;
        let mut res_queue: MaybeUninit<liblgmp_sys::PLGMPHostQueue> = MaybeUninit::zeroed();

        let res =
            unsafe { liblgmp_sys::lgmpHostQueueNew(host, config.into(), res_queue.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(res_queue)
            .map(|inner| LGMPHostQueue { inner })
    }

    pub fn mem_available(&self) -> usize {
        let host = self.inner;

        unsafe { liblgmp_sys::lgmpHostMemAvail(host) }
    }

    pub fn mem_alloc(&mut self, size: u32) -> LGMPResult<&LGMPMemoryAllocation> {
        self.mem_alloc_aligned(size, 4)
    }

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

pub struct LGMPHostQueue {
    inner: liblgmp_sys::PLGMPHostQueue,
}

impl LGMPHostQueue {
    pub fn has_subs(&self) -> bool {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueueHasSubs(queue) }
    }

    pub fn new_subs(&self) -> u32 {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueueNewSubs(queue) }
    }

    pub fn pending(&self) -> u32 {
        let queue = self.inner;
        unsafe { liblgmp_sys::lgmpHostQueuePending(queue) }
    }

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

    pub fn ack_data(&mut self) -> LGMPResult<()> {
        let queue = self.inner;

        let res = unsafe { liblgmp_sys::lgmpHostAckData(queue) };

        Status::from(res).ok_if_success(())
    }

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

pub struct LGMPMemoryAllocation {
    inner: liblgmp_sys::PLGMPMemory,
}

impl LGMPMemoryAllocation {
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
