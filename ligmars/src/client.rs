//! Structs and functions for connecting to an LGMP shared memory connction as a client. Requres the [client] feature enabled to use.
use std::mem::MaybeUninit;

use crate::{
    error::{LGMPResult, Status},
    shm_file::ShmFileHandle,
};

/// Handle to the client side of an SHM communication file
pub struct LGMPClient {
    source: Option<Box<dyn ShmFileHandle>>,
    inner: liblgmp_sys::PLGMPClient,
}

impl LGMPClient {
    /// Initialises a handle to the client side of a LGMP connection
    /// given a handle to a memory mapped SHM file.
    /// The SHM file and memory mapped region must be large enough to
    /// handle all communications, as it cannot be resized during use.
    pub fn init(mut file: Box<dyn ShmFileHandle>) -> LGMPResult<LGMPClient> {
        let ptr = file.get_mut_ptr() as *mut std::ffi::c_void;
        let size: usize = file.get_size();

        let mut client: LGMPClient = unsafe { Self::init_from_ptr(ptr, size)? };

        client.source = Some(file);
        Ok(client)
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

    /// Initialises a client session on the already-initialised client.
    ///
    /// This will return an error if a host has not already been initialised on the same SHM
    /// file.
    /// Returns both a copy of the udata byte array set by the host upon startup and
    /// the clientID assigned by the host.
    pub fn client_session_init(&mut self) -> LGMPResult<(Vec<u8>, u32)> {
        //TODO: check if udata needs to be written, in which case need to return pointer rather than copy
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

    /// Returns true if the session running on the current client is still valid.
    ///
    /// This may return false if the host has been restarted or if the last heartbeat
    /// recieved from the host has passed the timout (1000ms).
    pub fn client_session_valid(&self) -> bool {
        unsafe { liblgmp_sys::lgmpClientSessionValid(self.inner) }
    }

    /// Subscribe to the queue indicated by the provided ID.
    ///
    /// Returns a handle to the queue if it exists, may alternatively return `LGMPErrNoSuchQueue`
    /// if a queue does not exist at the provided ID.
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

/// A handle to the client end of a queue.
///
/// Whilst it will be cleaned up upon being dropped, unsubscribing from a channel is falliable
/// and if it does fail, resources may be leaked until the host cleans them up. If having
/// this happen automatically is undesirable, unsubscribe can be called directly allowing
/// you to manually handle any issues.
pub struct ClientQueueHandle {
    inner: *mut liblgmp_sys::LGMPClientQueue,
}

impl ClientQueueHandle {
    fn get_ptr(&mut self) -> *mut liblgmp_sys::PLGMPClientQueue {
        &mut self.inner as *mut liblgmp_sys::PLGMPClientQueue
    }

    /// Attempts to unsubscribe from the channel.
    pub fn unsubscribe(&mut self) -> LGMPResult<()> {
        let ptr = self.get_ptr();
        let res = unsafe { liblgmp_sys::lgmpClientUnsubscribe(ptr) };

        Status::from(res).ok_if_success(())
    }

    /// First attempts to unsubscribe from the channel, but if it doesn't work, forcibly
    /// drops it anyway
    ///
    /// # Safety
    /// This function may cause resources to be leaked if the unsubscribe function fails
    pub unsafe fn force_drop(mut self) {
        match self.unsubscribe() {
            Ok(_) => std::mem::drop(self),
            Err(_) => {
                // Unsubscribe failed, so we can assume that drop will not work.
                std::mem::forget(self)
            }
        }
    }

    /// Marks all messages except the most recent one in the queue as read.
    ///
    /// As such, all messages apart from the most recent will be discarded, and the
    /// next call to process will return the most recent message available (as of
    /// this function being called).
    pub fn advance_to_last(&mut self) -> LGMPResult<()> {
        let ptr = self.inner;
        let res = unsafe { liblgmp_sys::lgmpClientAdvanceToLast(ptr) };

        Status::from(res).ok_if_success(())
    }

    /// Returns the next unread message in this channel but does not mark it as read.
    pub fn process(&self) -> LGMPResult<Message> {
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

    /// Marks the first unread message in this channel as read.
    pub fn message_done(&mut self) -> LGMPResult<()> {
        let ptr = self.inner;
        let res = unsafe { liblgmp_sys::lgmpClientMessageDone(ptr) };

        Status::from(res).ok_if_success(())
    }

    /// Sends an array of bytes to the host over the channel. The host will then
    /// forward it to any listening clients on this channel.
    ///
    /// Returns the unique, incrementing ID assigned to this message.
    /// You cahn check if this message has been processed by the host
    /// by comparing this value with that returned by ```self.get_serial```.
    pub fn send_data(&mut self, mut data: Vec<u8>) -> LGMPResult<u32> {
        let queue = self.inner;
        let data_ptr = data.as_mut_ptr() as *mut std::ffi::c_void;
        let size = u32::try_from(data.len())?;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientSendData(queue, data_ptr, size, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }

    /// Returns the number of messages that have been processed by the host.
    ///
    /// This will also represent the serial number of the message most recently processed.
    /// As such, you can check that a message has been processed by comparing the value returned
    /// by the send_data function call to the serial here, for example
    /// ```
    /// let msg_serial = chan.send_data(data)?;
    /// if chan.get_serial()? >= msg_serial {
    ///     println!("Message recieved by host");
    /// } else {
    ///     println!("Message not yet recieved by host");
    /// }
    /// ```
    pub fn get_serial(&mut self) -> LGMPResult<u32> {
        let queue = self.inner;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientGetSerial(queue, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }
}

impl Drop for ClientQueueHandle {
    fn drop(&mut self) {
        let replacement = ClientQueueHandle {
            inner: std::ptr::null_mut::<liblgmp_sys::LGMPClientQueue>(),
        };
        let to_drop = std::mem::replace(self, replacement);
        unsafe { to_drop.force_drop() }
    }
}

/// Struct representing a message sent by a host, returned by successful calls to process
pub struct Message {
    /// User-defined data, may be used to hold eg. message identifiers
    pub udata: u32,
    /// Copy of the binary data sent by the host
    pub mem: Vec<u8>,
}
