//! Structs and functions for connecting to an LGMP shared memory connction as a client.
//! Requres the `client` feature enabled to use.
use std::{
    mem::MaybeUninit,
    ops::DerefMut,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::{
    error::{LGMPResult, Status},
    shm_file::ShmFileHandle,
};

/// Handle to the client side of an SHM communication file
pub struct Client {
    source: Option<Box<dyn ShmFileHandle>>,
    inner: liblgmp_sys::PLGMPClient,
}

unsafe impl Send for Client {}

impl Client {
    /// Initialises a handle to the client side of a LGMP connection
    /// given a handle to a memory mapped SHM file.
    /// The SHM file and memory mapped region must be large enough to
    /// handle all communications, as it cannot be resized during use.
    pub fn init(mut file: Box<dyn ShmFileHandle>) -> LGMPResult<Client> {
        let ptr = file.get_mut_ptr() as *mut std::ffi::c_void;
        let size: usize = file.get_size();

        let mut client: Client = unsafe { Self::init_from_ptr(ptr, size)? };

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
    unsafe fn init_from_ptr(mem: *mut std::ffi::c_void, size: usize) -> LGMPResult<Client> {
        let mut res_ptr: MaybeUninit<liblgmp_sys::PLGMPClient> = MaybeUninit::zeroed();
        let res: u32 = unsafe { liblgmp_sys::lgmpClientInit(mem, size, res_ptr.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(res_ptr)
            .map(|inner| Client {
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
                udata_vec.resize(udata_slice.len(), 0);
                udata_vec.copy_from_slice(udata_slice);
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
                inner: Arc::new(Mutex::new(queue_handle)),
            })
    }
}

impl Drop for Client {
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
    inner: Arc<Mutex<*mut liblgmp_sys::LGMPClientQueue>>,
}

impl ClientQueueHandle {
    /// Obtains the lock on internal channel pointer  and returns a pointer to it
    fn get_ptr(&self) -> LGMPResult<*mut liblgmp_sys::PLGMPClientQueue> {
        let mut inner = self.inner.lock()?;
        Ok(std::ops::DerefMut::deref_mut(&mut inner) as *mut liblgmp_sys::PLGMPClientQueue)
    }

    /// Attempts to unsubscribe from the channel.
    pub fn unsubscribe(&mut self) -> LGMPResult<()> {
        let ptr = self.get_ptr()?;
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
        let ptr = self.get_ptr()?;
        let res = unsafe { liblgmp_sys::lgmpClientAdvanceToLast(*ptr) };

        Status::from(res).ok_if_success(())
    }

    /// Returns a copy of the next unread message in this channel but does not mark it as read.
    /// Equivalent to `.process` in the original LGMP library
    pub fn peek(&self) -> LGMPResult<Message> {
        let ptr = self.get_ptr()?;
        let mut result: MaybeUninit<liblgmp_sys::LGMPMessage> = MaybeUninit::zeroed();

        let res = unsafe { liblgmp_sys::lgmpClientProcess(*ptr, result.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(result)
            .and_then(|msg| {
                let len = usize::try_from(msg.size)?;
                let data = unsafe { core::slice::from_raw_parts(msg.mem as *mut u8, len) };
                let mut mem = Vec::new();
                mem.resize(data.len(), 0);
                mem.copy_from_slice(data);

                Ok(Message {
                    udata: msg.udata,
                    mem,
                })
            })
    }

    /// Returns a copy of the next unread message in this channel and marks it as read.
    /// This will also invalidate any in-place pointers to messages
    pub fn pop(&mut self) -> LGMPResult<Message> {
        let msg = self.peek()?;
        self.message_done()?;
        Ok(msg)
    }

    /// Returns a struct containing a raw pointer to the block of shared memory
    /// sent as the next message, as well as the size of the memory block.
    ///
    /// This memory may be deallocated by the host at any time, although it should
    /// be retained until either the client channel times out or `message_done`
    /// is called.
    pub fn peek_raw(&self) -> LGMPResult<SharedMemoryBlock> {
        let ptr = self.get_ptr()?;
        let mut result: MaybeUninit<liblgmp_sys::LGMPMessage> = MaybeUninit::zeroed();

        let res = unsafe { liblgmp_sys::lgmpClientProcess(*ptr, result.as_mut_ptr()) };

        Status::from(res)
            .ok_and_init_if_success(result)
            .and_then(|msg| {
                let len = usize::try_from(msg.size)?;

                Ok(SharedMemoryBlock {
                    udata: msg.udata,
                    mem: msg.mem,
                    size: len,
                })
            })
    }

    /// Returns a struct containing a raw pointer to the block of shared memory
    /// sent as the next message, as well as the size of the memory block.
    ///
    /// A lock will also be held on the channel struct pointer, preventing
    /// this client from calling `message_done` on the channel, ensuring that
    /// this pointer should remain valid until dropped.
    pub fn peek_in_place(&self) -> LGMPResult<InPlaceMessage> {
        let block = self.peek_raw()?;
        let msg = InPlaceMessage {
            mem: block,
            _channel_handle: self.inner.lock()?,
            _should_mark_done: false,
        };

        Ok(msg)
    }

    /// Returns a struct containing a raw pointer to the block of shared memory
    /// sent as the next message, as well as the size of the memory block.
    ///
    /// A lock will also be held on the channel struct pointer, preventing
    /// this client from calling `message_done` on the channel, ensuring that
    /// this pointer should remain valid until dropped.
    ///
    /// `message_done` will also be called on the channel upon drop, thereby
    /// advancing the queue.
    pub fn pop_in_place(&mut self) -> LGMPResult<InPlaceMessage> {
        let block = self.peek_raw()?;
        let msg = InPlaceMessage {
            mem: block,
            _channel_handle: self.inner.lock()?,
            _should_mark_done: true,
        };

        Ok(msg)
    }

    /// Marks the first unread message in this channel as read.
    ///
    /// This will also invalidate any in-place pointers to messages
    pub fn message_done(&mut self) -> LGMPResult<()> {
        let ptr = self.get_ptr()?;
        let res = unsafe { liblgmp_sys::lgmpClientMessageDone(*ptr) };

        Status::from(res).ok_if_success(())
    }

    /// Sends an array of bytes to the host over the channel. The host will then
    /// forward it to any listening clients on this channel.
    ///
    /// Returns the unique, incrementing ID assigned to this message.
    /// You cahn check if this message has been processed by the host
    /// by comparing this value with that returned by ```self.get_serial```.
    pub fn send_data(&mut self, mut data: Vec<u8>) -> LGMPResult<u32> {
        let queue = self.get_ptr()?;
        let data_ptr = data.as_mut_ptr() as *mut std::ffi::c_void;
        let size = u32::try_from(data.len())?;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientSendData(*queue, data_ptr, size, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }

    /// Returns the number of messages that have been processed by the host.
    ///
    /// This will also represent the serial number of the message most recently processed.
    /// As such, you can check that a message has been processed by comparing the value returned
    /// by the send_data function call to the serial here, for example
    /// ```ignore
    /// let msg_serial = chan.send_data(data)?;
    /// if chan.get_serial()? >= msg_serial {
    ///     println!("Message recieved by host");
    /// } else {
    ///     println!("Message not yet recieved by host");
    /// }
    /// # Ok::<(), ligmars::error::Error>(())
    /// ```
    pub fn get_serial(&mut self) -> LGMPResult<u32> {
        let queue = self.get_ptr()?;
        let mut serial: u32 = 0;

        let res = unsafe { liblgmp_sys::lgmpClientGetSerial(*queue, &mut serial) };

        Status::from(res).ok_if_success(serial)
    }
}

impl Drop for ClientQueueHandle {
    fn drop(&mut self) {
        if let Err(e) = self.unsubscribe() {
            eprintln!(
                "Failed to unsubscribe from client queue due to error {:?}, dropping anyway...",
                e
            )
        }
    }
}

/// Struct representing a message sent by a host, returned by successful calls to process
pub struct Message {
    /// User-defined data, may be used to hold eg. message identifiers
    pub udata: u32,
    /// Copy of the binary data sent by the host
    pub mem: Vec<u8>,
}

/// Raw pointer to block of shared memory
pub struct SharedMemoryBlock {
    /// User-defined data, may be used to hold eg. message identifiers
    pub udata: u32,
    /// Pointer to the shared data sent by the host
    pub mem: *mut std::ffi::c_void,
    /// Size of the shared data region in bytes
    pub size: usize,
}

impl SharedMemoryBlock {
    /// Returns the contents of the referenced shared memory block as a
    /// slice of bytes.
    ///
    /// # Safety
    /// This memory block could be deallocated and reused by the host. This
    /// shouldn't happen as long as `done` is not called on the channel which sent
    /// this message, but it cannot be guaranteed.
    pub unsafe fn as_slice(&self) -> &[u8] {
        let len = self.size;
        unsafe { core::slice::from_raw_parts(self.mem as *mut u8, len) }
    }
}

pub struct InPlaceMessage<'a> {
    /// Pointer to the shared data sent by the host
    pub mem: SharedMemoryBlock,
    /// Lock on the channel
    _channel_handle: MutexGuard<'a, *mut liblgmp_sys::LGMPClientQueue>,
    /// Whether or not we should mark the message as done upon drop
    _should_mark_done: bool,
}

impl std::ops::Deref for InPlaceMessage<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        // Should be safe as we have a lock on the channel
        unsafe { self.mem.as_slice() }
    }
}

impl Drop for InPlaceMessage<'_> {
    fn drop(&mut self) {
        // Advance  channel if requested
        if self._should_mark_done {
            let chan = self._channel_handle.deref_mut();
            let res = unsafe { liblgmp_sys::lgmpClientMessageDone(*chan) };

            if let Err(e) = Status::from(res).ok_if_success(()) {
                eprintln!("Failed to mark message as done due to error {:?}", e)
            }
        }
    }
}
