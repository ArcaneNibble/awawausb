use std::ffi::{OsString, c_void};
use std::io;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::mpsc;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Properties::*;
use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::core::*;

fn multi_sz_to_list(inp: &[u16]) -> Vec<crate::NullU16Path> {
    let mut ret = inp
        .split(|x| *x == 0)
        .map(|x| crate::NullU16Path::from(x))
        .collect::<Vec<_>>();

    // Remove up to two training empty lists, because \x00\x00 end
    if let Some(last) = ret.last()
        && last.0 == [0]
    {
        ret.pop();
    }
    if let Some(last) = ret.last()
        && last.0 == [0]
    {
        ret.pop();
    }

    ret
}

pub fn find_instance_paths(
    guid: &GUID,
    dev_inst_id: Option<&[u16]>,
) -> Result<Vec<crate::NullU16Path>, u32> {
    let dev_inst_id = if let Some(x) = dev_inst_id {
        x.as_ptr()
    } else {
        ptr::null()
    };

    loop {
        let mut list_sz = 0;
        let ret = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut list_sz,
                guid,
                dev_inst_id,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if ret != CR_SUCCESS {
            return Err(ret);
        }

        let mut buf = vec![0; list_sz as usize];
        let ret = unsafe {
            CM_Get_Device_Interface_ListW(
                guid,
                dev_inst_id,
                buf.as_mut_ptr(),
                list_sz,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if ret == CR_BUFFER_SMALL {
            continue;
        }
        if ret != CR_SUCCESS {
            return Err(ret);
        }

        return Ok(multi_sz_to_list(&buf));
    }
}

fn get_devnode_str_property(devnode: u32, key: &DEVPROPKEY) -> Result<Vec<u16>, u32> {
    let mut buf_sz = 0;
    let mut ptype = 0;
    let ret = unsafe {
        CM_Get_DevNode_PropertyW(devnode, key, &mut ptype, ptr::null_mut(), &mut buf_sz, 0)
    };
    if ret != CR_BUFFER_SMALL {
        return Err(ret);
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(CR_INVALID_PROPERTY);
    }

    let mut buf = vec![0u16; (buf_sz as usize + 1) / 2];
    let ret = unsafe {
        CM_Get_DevNode_PropertyW(
            devnode,
            key,
            &mut ptype,
            buf.as_mut_ptr() as *mut u8,
            &mut buf_sz,
            0,
        )
    };
    if ret != CR_SUCCESS {
        return Err(ret);
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(CR_INVALID_PROPERTY);
    }

    unsafe {
        buf.set_len(buf_sz as usize / 2);
    }

    Ok(buf)
}

fn get_devnode_guid_property(devnode: u32, key: &DEVPROPKEY) -> Result<Vec<u8>, u32> {
    let mut buf_sz = 0;
    let mut ptype = 0;
    let ret = unsafe {
        CM_Get_DevNode_PropertyW(devnode, key, &mut ptype, ptr::null_mut(), &mut buf_sz, 0)
    };
    if ret != CR_BUFFER_SMALL {
        return Err(ret);
    }
    if ptype != DEVPROP_TYPE_GUID {
        return Err(CR_INVALID_PROPERTY);
    }

    let mut buf = vec![0; buf_sz as usize];
    let ret = unsafe {
        CM_Get_DevNode_PropertyW(devnode, key, &mut ptype, buf.as_mut_ptr(), &mut buf_sz, 0)
    };
    if ret != CR_SUCCESS {
        return Err(ret);
    }
    if ptype != DEVPROP_TYPE_GUID {
        return Err(CR_INVALID_PROPERTY);
    }

    unsafe {
        buf.set_len(buf_sz as usize);
    }

    Ok(buf)
}

fn probe_usb_device(instance_path: &[u16]) {
    let mut buf_sz = 0;
    let mut ptype = 0;
    let ret = unsafe {
        CM_Get_Device_Interface_PropertyW(
            instance_path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut ptype,
            ptr::null_mut(),
            &mut buf_sz,
            0,
        )
    };
    dbg!(ret);

    let mut buf = vec![0u16; (buf_sz as usize + 1) / 2];
    let ret = unsafe {
        CM_Get_Device_Interface_PropertyW(
            instance_path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut ptype,
            buf.as_mut_ptr() as *mut u8,
            &mut buf_sz,
            0,
        )
    };
    dbg!(ret);

    let buf_dbg = OsString::from_wide(&buf);
    dbg!(buf_dbg);

    let mut devnode = 0;
    let ret = unsafe { CM_Locate_DevNodeW(&mut devnode, buf.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
    dbg!(ret, devnode);

    //

    let driver = get_devnode_str_property(devnode, &DEVPKEY_Device_Service).unwrap();
    dbg!(OsString::from_wide(&driver));

    //

    let mut parent = 0;
    let ret = unsafe { CM_Get_Parent(&mut parent, devnode, 0) };
    dbg!(ret, parent);

    let driver = get_devnode_str_property(parent, &DEVPKEY_Device_Service).unwrap();
    dbg!(OsString::from_wide(&driver));

    let inst_id = get_devnode_str_property(parent, &DEVPKEY_Device_InstanceId).unwrap();
    dbg!(OsString::from_wide(&inst_id));

    let parent_paths = find_instance_paths(&GUID_DEVINTERFACE_USB_HUB, Some(&inst_id)).unwrap();
    dbg!(parent_paths);

    //

    let mut child = 0;
    let ret = unsafe { CM_Get_Child(&mut child, devnode, 0) };
    dbg!(ret, child);

    let driver = get_devnode_str_property(child, &DEVPKEY_Device_Service).unwrap();
    dbg!(OsString::from_wide(&driver));
}

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

    let instance_path = unsafe {
        if (*event_data).FilterType != CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE {
            log::warn!("Somehow got a bad CfgMgr32 notification type???");
            return ERROR_SUCCESS;
        }

        let iface = &(*event_data).u.DeviceInterface;
        let link_sz = event_data_sz as usize
            - mem::offset_of!(CM_NOTIFY_EVENT_DATA, u.DeviceInterface.SymbolicLink);
        let link_ptr = iface.SymbolicLink.as_ptr();
        std::slice::from_raw_parts(link_ptr, link_sz / 2)
    };

    dbg!(action);
    dbg!(crate::DbgU16(instance_path));

    if action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL {
        probe_usb_device(instance_path);
    }

    tx.send(()).unwrap();
    unsafe {
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
