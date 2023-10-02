use std::mem::MaybeUninit;

use crate::{
    error::{LGMPResult, Status},
    shm_file::ShmFile,
};

pub struct LGMPClient {
    source: Option<ShmFile>,
    inner: liblgmp_sys::PLGMPClient,
}

impl LGMPClient {
    pub fn init(file: ShmFile) -> LGMPResult<LGMPClient> {
        let ptr = file.get_ptr() as *mut std::ffi::c_void;
        let size: usize = file.file_size.try_into()?;

        let mut client: LGMPClient = unsafe { Self::init_from_ptr(ptr, size)? };

        client.source = Some(file);
        Ok(client)
    }

    unsafe fn init_from_ptr(mem: *mut std::ffi::c_void, size: usize) -> LGMPResult<LGMPClient> {
        let mut res_ptr: MaybeUninit<liblgmp_sys::PLGMPClient> = MaybeUninit::zeroed();
        let res: u32 = unsafe { liblgmp_sys::lgmpClientInit(mem, size, res_ptr.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(res_ptr)
            .map(|inner| LGMPClient {
                inner,
                source: None,
            })
    }

    //TODO: check if udata needs to be written, in which case need to return pointer rather than copy
    pub fn client_session_init(&mut self) -> LGMPResult<(Vec<u8>, u32)> {
        let mut udata_size_raw: u32 = 0;
        let mut udata_ptr: MaybeUninit<*mut u8> = MaybeUninit::zeroed();
        let mut client_id: u32 = 0;
        let client = self.inner;

        let res: u32 = unsafe {
            liblgmp_sys::lgmpClientSessionInit(
                client,
                &mut udata_size_raw,
                udata_ptr.as_mut_ptr(),
                &mut client_id,
            )
        };

        let udata_size: usize = usize::try_from(udata_size_raw)?;

        Status::from(res)
            .ok_and_init_if_success(udata_ptr)
            .map(|udata| {
                let udata_slice: &[u8];
                unsafe {
                    udata_slice = std::slice::from_raw_parts(udata, udata_size);
                }
                let mut udata_vec = Vec::new();
                udata_vec.clone_from_slice(udata_slice);
                (udata_vec, client_id)
            })
    }

    pub fn client_session_valid(&self) -> bool {
        unsafe { liblgmp_sys::lgmpClientSessionValid(self.inner) }
    }

    pub fn client_subscribe(&mut self, queue_id: u32) -> LGMPResult<ClientQueueHandle> {
        let mut queue_ptr: MaybeUninit<*mut liblgmp_sys::LGMPClientQueue> = MaybeUninit::zeroed();
        let client = self.inner;

        let res: u32 =
            unsafe { liblgmp_sys::lgmpClientSubscribe(client, queue_id, queue_ptr.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(queue_ptr)
            .map(|queue_handle| ClientQueueHandle {
                inner: queue_handle,
            })
    }
}

impl Drop for LGMPClient {
    fn drop(&mut self) {
        //Call free on internal struct
        unsafe {
            liblgmp_sys::lgmpClientFree(&mut self.inner);
        }
    }
}

pub struct ClientQueueHandle {
    inner: *mut liblgmp_sys::LGMPClientQueue,
}

impl ClientQueueHandle {
    fn get_ptr(&mut self) -> *mut liblgmp_sys::PLGMPClientQueue {
        &mut self.inner as *mut liblgmp_sys::PLGMPClientQueue
    }

    pub fn client_unsubscribe(&mut self) -> LGMPResult<()> {
        let ptr = self.get_ptr();
        let res = unsafe { liblgmp_sys::lgmpClientUnsubscribe(ptr) };

        Status::from(res).ok_if_success(())
    }

    pub fn client_advance_to_last(&mut self) -> LGMPResult<()> {
        let ptr = self.inner;
        let res = unsafe { liblgmp_sys::lgmpClientAdvanceToLast(ptr) };

        Status::from(res).ok_if_success(())
    }

    pub fn client_proces(&mut self) -> LGMPResult<Message> {
        let ptr = self.inner;
        let mut result: MaybeUninit<liblgmp_sys::LGMPMessage> = MaybeUninit::zeroed();

        let res = unsafe { liblgmp_sys::lgmpClientProcess(ptr, result.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(result)
            .and_then(|msg| {
                let len = usize::try_from(msg.size)?;
                let data = unsafe { core::slice::from_raw_parts(msg.mem as *mut u8, len) };
                let mut mem = Vec::new();
                mem.clone_from_slice(data);

                Ok(Message {
                    udata: msg.udata,
                    mem,
                })
            })
    }

    pub fn client_message_done(&mut self) -> LGMPResult<()> {
        let ptr = self.inner;
        let res = unsafe { liblgmp_sys::lgmpClientMessageDone(ptr) };

        Status::from(res).ok_if_success(())
    }

    pub fn client_send_data(&mut self, mut data: Vec<u8>) -> LGMPResult<u32> {
        let queue = self.inner;
        let data_ptr = data.as_mut_ptr() as *mut std::ffi::c_void;
        let size = u32::try_from(data.len())?;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientSendData(queue, data_ptr, size, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }

    pub fn client_get_serial(&mut self) -> LGMPResult<u32> {
        let queue = self.inner;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientGetSerial(queue, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }
}

impl Drop for ClientQueueHandle {
    fn drop(&mut self) {
        //unsubscribe, panicking if unsubscribe fails
        self.client_unsubscribe()
            .expect("Failed to unsubscribe from client channel")
    }
}

pub struct Message {
    pub udata: u32,
    pub mem: Vec<u8>,
}
