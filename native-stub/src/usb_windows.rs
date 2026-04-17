use std::ffi::{OsStr, c_void};
use std::fmt::Debug;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::{io, mem};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::{GetQueuedCompletionStatus, OVERLAPPED, OVERLAPPED_0};
use windows_sys::Win32::System::Threading::INFINITE;

pub fn iocp_thread(iocp: usize) {
    let iocp = iocp as HANDLE;
    loop {
        log::debug!("iocp thread");

        let mut nbytes = 0;
        let mut completion_key = 0;
        let mut lpoverlapped = ptr::null_mut();

        let ret = unsafe {
            GetQueuedCompletionStatus(
                iocp,
                &mut nbytes,
                &mut completion_key,
                &mut lpoverlapped,
                INFINITE,
            )
        };
        let mut status = 0;
        if ret == 0 {
            if lpoverlapped.is_null() {
                // > the function did not dequeue a completion packet from the completion port
                log::error!("IOCP polling failed! {}", io::Error::last_os_error());
                break;
            } else {
                // > the function dequeues a completion packet
                // > for a **failed** I/O operation from the completion port
                status = io::Error::last_os_error().raw_os_error().unwrap() as u32;
            }
        }

        if completion_key == 1 {
            log::debug!("IOCP thread exiting!");
            break;
        }

        // An "actual" completion
        let mut urbwrapper = unsafe {
            let urb_wrapper_ptr = (lpoverlapped as *mut u8)
                .byte_offset(-(mem::offset_of!(WindowsURBWrapper, overlapped) as isize));
            let urb_wrapper_ptr = urb_wrapper_ptr as *mut WindowsURBWrapper;
            Box::from_raw(urb_wrapper_ptr)
        };

        unsafe {
            urbwrapper.buf.set_len(nbytes as usize);
        }

        log::debug!(
            "request {} finished, status {}, buf {:02x?}",
            urbwrapper.txn_id,
            status,
            urbwrapper.buf,
        );

        // Send notification
        if status == ERROR_GEN_FAILURE {
            // This is a stall, apparently
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: urbwrapper.txn_id.clone(),
                error: crate::protocol::Errors::Stall,
                bytes_written: nbytes as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_windows::write_stdout_msg(notif.as_bytes())
                .expect("failed to write stdout");
        } else if status == 0 {
            // FIXME: Does Windows just not detect a babble condition?
            let babble = false;
            let data: Option<_> = if urbwrapper.dir == crate::USBTransferDirection::DeviceToHost {
                Some(URL_SAFE_NO_PAD.encode(&urbwrapper.buf))
            } else {
                None
            };
            let notif = crate::protocol::ResponseMessage::RequestComplete {
                txn_id: urbwrapper.txn_id.clone(),
                babble,
                data,
                bytes_written: nbytes as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_windows::write_stdout_msg(notif.as_bytes())
                .expect("failed to write stdout");
        } else {
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: urbwrapper.txn_id.clone(),
                error: crate::protocol::Errors::TransferError,
                bytes_written: nbytes as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_windows::write_stdout_msg(notif.as_bytes())
                .expect("failed to write stdout");
        }
    }
}

pub fn zero_overlapped() -> OVERLAPPED {
    OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Pointer: ptr::null_mut(),
        },
        hEvent: ptr::null_mut(),
    }
}

pub struct WindowsURBWrapper {
    pub txn_id: String,
    pub dir: crate::USBTransferDirection,
    pub buf: Vec<u8>,
    pub overlapped: OVERLAPPED,
}
impl Debug for WindowsURBWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsURBWrapper")
            .field("txn_id", &self.txn_id)
            .field("dir", &self.dir)
            .field("buf", &self.buf)
            .field("overlapped.Internal", &self.overlapped.Internal)
            .field("overlapped.InternalHigh", &self.overlapped.InternalHigh)
            .field("overlapped.Pointer", unsafe {
                &self.overlapped.Anonymous.Pointer
            })
            .field("overlapped.hEvent", &self.overlapped.hEvent)
            .finish()
    }
}

#[derive(Debug)]
pub struct WinUSBHandle {
    pub raw_handle: HANDLE,
    winusb_handle: WINUSB_INTERFACE_HANDLE,
}
impl Drop for WinUSBHandle {
    fn drop(&mut self) {
        unsafe {
            WinUsb_Free(self.winusb_handle);
            if self.raw_handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.raw_handle);
            }
        }
    }
}
impl WinUSBHandle {
    pub fn open(path: &OsStr) -> io::Result<Self> {
        let path = path.encode_wide().chain(Some(0)).collect::<Vec<_>>();

        let hfile = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if hfile == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        let mut hwinusb = ptr::null_mut();
        let ret = unsafe { WinUsb_Initialize(hfile, &mut hwinusb) };
        if ret == 0 {
            unsafe { CloseHandle(hfile) };
            return Err(io::Error::last_os_error());
        }

        // Disable timeouts on control port
        let mut ret = Self {
            raw_handle: hfile,
            winusb_handle: hwinusb,
        };
        ret._set_timeout(0);

        Ok(ret)
    }

    pub fn open_other(&mut self, other_idx: u8) -> io::Result<Self> {
        let mut hwinusb = ptr::null_mut();
        let ret =
            unsafe { WinUsb_GetAssociatedInterface(self.winusb_handle, other_idx, &mut hwinusb) };
        if ret == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            raw_handle: INVALID_HANDLE_VALUE,
            winusb_handle: hwinusb,
        })
    }

    pub fn set_alt_if(&mut self, alt: u8) -> io::Result<()> {
        let ret = unsafe { WinUsb_SetCurrentAlternateSetting(self.winusb_handle, alt) };
        if ret == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn clear_halt(&mut self, ep: u8) -> io::Result<()> {
        let ret = unsafe { WinUsb_ResetPipe(self.winusb_handle, ep) };
        if ret == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn _set_timeout(&mut self, timeout: u32) {
        let timeout = timeout as u32;
        let ret = unsafe {
            WinUsb_SetPipePolicy(
                self.winusb_handle,
                0,
                PIPE_TRANSFER_TIMEOUT,
                4,
                &timeout as *const _ as *const c_void,
            )
        };
        if ret == 0 {
            log::warn!(
                "Failed to set control transfer timeout to {}! {}",
                timeout,
                io::Error::last_os_error()
            );
            // Proceed anyways
        }
    }

    pub fn ctrl_xfer(
        &mut self,
        txn_id: &str,
        mut buf: Vec<u8>,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: u32,
        dir: crate::USBTransferDirection,
    ) -> io::Result<()> {
        assert!(length as usize <= buf.capacity());

        // XXX We should "always" be doing this, but the handle used for internal transfers is immediately closed
        if timeout != 0 {
            self._set_timeout(timeout);
        }

        let buf_ptr = buf.as_mut_ptr();
        let urbwrapper = Box::new(WindowsURBWrapper {
            txn_id: txn_id.to_owned(),
            dir,
            buf,
            overlapped: zero_overlapped(),
        });
        let lpoverlapped = &urbwrapper.overlapped as *const OVERLAPPED;
        let _urbwrapper = Box::into_raw(urbwrapper);

        let setup = WINUSB_SETUP_PACKET {
            RequestType: request_type,
            Request: request,
            Value: value,
            Index: index,
            Length: length,
        };

        let mut _transferred = 0;
        unsafe {
            WinUsb_ControlTransfer(
                self.winusb_handle,
                setup,
                buf_ptr,
                length as u32,
                &mut _transferred,
                lpoverlapped,
            )
        };

        // This never "succeeds"
        let err = io::Error::last_os_error();
        if err.raw_os_error().unwrap() as u32 != ERROR_IO_PENDING {
            return Err(err);
        }

        Ok(())
    }

    pub fn data_xfer(
        &mut self,
        txn_id: &str,
        mut buf: Vec<u8>,
        ep: u8,
        length: u32,
        dir: crate::USBTransferDirection,
    ) -> io::Result<()> {
        assert!(length as usize <= buf.capacity());

        let buf_ptr = buf.as_mut_ptr();
        let urbwrapper = Box::new(WindowsURBWrapper {
            txn_id: txn_id.to_owned(),
            dir,
            buf,
            overlapped: zero_overlapped(),
        });
        let lpoverlapped = &urbwrapper.overlapped as *const OVERLAPPED;
        let _urbwrapper = Box::into_raw(urbwrapper);

        match dir {
            crate::USBTransferDirection::HostToDevice => unsafe {
                WinUsb_WritePipe(
                    self.winusb_handle,
                    ep,
                    buf_ptr as *const u8,
                    length,
                    ptr::null_mut(),
                    lpoverlapped,
                )
            },
            crate::USBTransferDirection::DeviceToHost => unsafe {
                WinUsb_ReadPipe(
                    self.winusb_handle,
                    ep,
                    buf_ptr,
                    length,
                    ptr::null_mut(),
                    lpoverlapped,
                )
            },
        };

        // This never "succeeds"
        let err = io::Error::last_os_error();
        if err.raw_os_error().unwrap() as u32 != ERROR_IO_PENDING {
            return Err(err);
        }

        Ok(())
    }
}
