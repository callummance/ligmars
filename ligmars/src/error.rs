//! Types used to represent different LGMP results
use std::mem::MaybeUninit;

use num_enum::{FromPrimitive, IntoPrimitive};
use thiserror::Error;

#[allow(missing_docs)]
/// Status returned by the LGMP C library
#[derive(Debug, Error, Eq, PartialEq, IntoPrimitive, FromPrimitive)]
#[repr(u32)]
pub enum Status {
    #[error("Success returned")]
    LGMPStatusOk = liblgmp_sys::LGMP_STATUS_LGMP_OK,
    #[error("SHMEM communication clock failure")]
    LGMPErrClockFailure = liblgmp_sys::LGMP_STATUS_LGMP_ERR_CLOCK_FAILURE,
    #[error("Invalid argument")]
    LGMPErrInvalidArgument = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_ARGUMENT,
    #[error("Invalid size")]
    LGMPErrInvalidSize = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_SIZE,
    #[error("Invalid memory alignment")]
    LGMPErrInvalidAlignment = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_ALIGNMENT,
    #[error("Invalid LGMP session")]
    LGMPErrInvalidSession = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_SESSION,
    #[error("Memory allocation failed")]
    LGMPErrNoMem = liblgmp_sys::LGMP_STATUS_LGMP_ERR_NO_MEM,
    #[error("Insufficient space in shared memory region")]
    LGMPErrNoSharedMem = liblgmp_sys::LGMP_STATUS_LGMP_ERR_NO_SHARED_MEM,
    #[error("Host already started")]
    LGMPErrHostStarted = liblgmp_sys::LGMP_STATUS_LGMP_ERR_HOST_STARTED,
    #[error("The maximum number of LGMP communication queues has already been opened")]
    LGMPErrNoQueues = liblgmp_sys::LGMP_STATUS_LGMP_ERR_NO_QUEUES,
    #[error("Unable to send message as LGMP queue was full")]
    LGMPErrQueueFull = liblgmp_sys::LGMP_STATUS_LGMP_ERR_QUEUE_FULL,
    #[error("LGMP queue was empty")]
    LGMPErrQueueEmpty = liblgmp_sys::LGMP_STATUS_LGMP_ERR_QUEUE_EMPTY,
    #[error("LGMP queue was already unsubscribed from")]
    LGMPErrQueueUnsubscribed = liblgmp_sys::LGMP_STATUS_LGMP_ERR_QUEUE_UNSUBSCRIBED,
    #[error("LGMP queue timed out")]
    LGMPErrQueueTimeout = liblgmp_sys::LGMP_STATUS_LGMP_ERR_QUEUE_TIMEOUT,
    #[error("Invalid magic number found")]
    LGMPErrInvalidMagic = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_MAGIC,
    #[error("Remote version did not match")]
    LGMPErrInvalidVersion = liblgmp_sys::LGMP_STATUS_LGMP_ERR_INVALID_VERSION,
    #[error("Requested LGMP queue did not exist")]
    LGMPErrNoSuchQueue = liblgmp_sys::LGMP_STATUS_LGMP_ERR_NO_SUCH_QUEUE,
    #[error("LGMP message was corrupted")]
    LGMPErrCorrupted = liblgmp_sys::LGMP_STATUS_LGMP_ERR_CORRUPTED,
    #[num_enum(catch_all)]
    #[error("Unexpected error code {0} returned by LGMP library")]
    LgmpUnknownErr(u32),
}
impl Status {
    pub(crate) fn ok_if_success<T>(self, ret_val: T) -> LGMPResult<T> {
        match self {
            Self::LGMPStatusOk => Ok(ret_val),
            e => Err(Error::InternalError(e)),
        }
    }

    pub(crate) fn ok_and_init_if_success<T>(self, ret_val: MaybeUninit<T>) -> LGMPResult<T> {
        match self {
            Self::LGMPStatusOk => {
                let ret_init: T;
                unsafe {
                    ret_init = ret_val.assume_init();
                }
                Ok(ret_init)
            }
            e => Err(Error::InternalError(e)),
        }
    }
}

#[allow(missing_docs)]
/// Error type returned by wrapper functions
#[derive(Error, Debug)]
pub enum Error {
    #[error("Internal library returned status {0}")]
    InternalError(#[from] Status),
    #[error("Encountered IO error whilst opening shared memory: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Failed to convert size to the format expected by internal library")]
    ConversionError(#[from] std::num::TryFromIntError),
}

/// Alias for result type returned by wrapper functions
pub type LGMPResult<T> = Result<T, Error>;

impl From<Status> for LGMPResult<()> {
    fn from(value: Status) -> Self {
        value.ok_if_success(())
    }
}
