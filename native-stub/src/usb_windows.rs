use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::GetQueuedCompletionStatus;
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
        if ret == 0 {
            log::error!("IOCP polling failed! {}", io::Error::last_os_error());
            break;
        }

        if completion_key == 1 {
            log::debug!("IOCP thread exiting!");
            break;
        }

        dbg!(nbytes, lpoverlapped);
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
        let zero = 0u32;
        let ret = unsafe {
            WinUsb_SetPipePolicy(
                hwinusb,
                0,
                PIPE_TRANSFER_TIMEOUT,
                4,
                &zero as *const _ as *const c_void,
            )
        };
        if ret == 0 {
            log::warn!(
                "Failed to disable control transfer timeout! {}",
                io::Error::last_os_error()
            );
            // Proceed anyways
        }

        Ok(Self {
            raw_handle: hfile,
            winusb_handle: hwinusb,
        })
    }
}
