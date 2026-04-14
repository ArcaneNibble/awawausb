use std::ffi::{OsString, c_void};
use std::io;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::mpsc;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Threading::*;

unsafe extern "system" fn notify_cb(
    _hnotify: HCMNOTIFICATION,
    ctx: *const std::ffi::c_void,
    action: i32,
    event_data: *const CM_NOTIFY_EVENT_DATA,
    event_data_sz: u32,
) -> u32 {
    // NOTE: This function executes on a thread pool thread and not the main thread
    let (tx, hevent) = unsafe {
        let tx = &((*(ctx as *const WinNotificationHandler)).tx);
        let hevent = (*(ctx as *const WinNotificationHandler)).h_event;
        (tx.clone(), hevent)
    };

    dbg!(action);
    unsafe {
        dbg!((*event_data).FilterType);

        let link_sz = event_data_sz as usize
            - mem::offset_of!(CM_NOTIFY_EVENT_DATA, u.DeviceInterface.SymbolicLink);
        dbg!(link_sz);

        let iface = &(*event_data).u.DeviceInterface;
        let link_ptr = iface.SymbolicLink.as_ptr();
        let link = std::slice::from_raw_parts(link_ptr, link_sz / 2);
        let link_dbg = OsString::from_wide(&link);
        dbg!(link_dbg);

        tx.send(()).unwrap();
        SetEvent(hevent);
    }

    ERROR_SUCCESS
}

#[derive(Debug)]
pub struct WinNotificationHandler {
    h_notif: HCMNOTIFICATION,
    pub h_event: HANDLE,
    tx: mpsc::Sender<()>,
    rx: mpsc::Receiver<()>,
}
impl WinNotificationHandler {
    // Takes a pinned raw pointer to an *uninitialized* Self
    pub unsafe fn new(self_: *mut Self) {
        let event = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        if event.is_null() {
            panic!(
                "Failed to set up notification event! {}",
                io::Error::last_os_error()
            );
        }

        let notif_filter = CM_NOTIFY_FILTER {
            cbSize: mem::size_of::<CM_NOTIFY_FILTER>() as u32,
            Flags: 0,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            Reserved: 0,
            u: CM_NOTIFY_FILTER_0 {
                DeviceInterface: CM_NOTIFY_FILTER_0_0 {
                    ClassGuid: GUID_DEVINTERFACE_USB_DEVICE,
                },
            },
        };
        let mut h_notify_context = INVALID_HANDLE_VALUE;
        let ret = unsafe {
            CM_Register_Notification(
                &notif_filter,
                self_ as *const c_void,
                Some(notify_cb),
                &mut h_notify_context,
            )
        };

        if ret != CR_SUCCESS {
            panic!(
                "Failed to set up device notification! {}",
                io::Error::last_os_error()
            );
        }

        let (tx, rx) = mpsc::channel();

        unsafe {
            (*self_).h_notif = h_notify_context;
            (*self_).h_event = event;
            ptr::addr_of_mut!((*self_).tx).write(tx);
            ptr::addr_of_mut!((*self_).rx).write(rx);
        }
    }
}
impl Drop for WinNotificationHandler {
    fn drop(&mut self) {
        unsafe {
            CM_Unregister_Notification(self.h_notif);
            CloseHandle(self.h_event);
        }
    }
}
