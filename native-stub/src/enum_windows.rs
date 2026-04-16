use std::error;
use std::ffi::{OsString, c_void};
use std::fmt::{Debug, Display};
use std::io;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::mpsc;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Properties::*;
use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Registry::{REG_MULTI_SZ, REG_SZ};
use windows_sys::Win32::System::Threading::*;
use windows_sys::core::*;

use crate::NullU16;

struct UnfuckedGUID(GUID);
impl Debug for UnfuckedGUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{")?;
        write!(f, "{:08x}-", self.0.data1)?;
        write!(f, "{:04x}-", self.0.data2)?;
        write!(f, "{:04x}-", self.0.data3)?;
        write!(f, "{:02x}{:02x}-", self.0.data4[0], self.0.data4[1])?;
        for i in 0..6 {
            write!(f, "{:02x}", self.0.data4[2 + i])?;
        }
        write!(f, "}}")?;
        Ok(())
    }
}
impl PartialEq for UnfuckedGUID {
    fn eq(&self, other: &Self) -> bool {
        (self.0.data1 == other.0.data1)
            && (self.0.data2 == other.0.data2)
            && (self.0.data3 == other.0.data3)
            && (self.0.data4 == other.0.data4)
    }
}
impl Eq for UnfuckedGUID {}

#[derive(Debug)]
pub enum WinEnumerationError {
    CfgMgr32Error(CfgMgrError),
    UnexpectedPropertyType(&'static str),
    CouldNotFindHub,
}
impl error::Error for WinEnumerationError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match self {
            Self::CfgMgr32Error(e) => Some(e),
            _ => None,
        }
    }
}
impl Display for WinEnumerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CfgMgr32Error(e) => write!(f, "CfgMgr32 returned {}", e),
            Self::UnexpectedPropertyType(msg) => {
                write!(f, "tried to read {} but it had the wrong type", msg)
            }
            Self::CouldNotFindHub => write!(f, "Could not find the hub owning the device"),
        }
    }
}
impl From<CfgMgrError> for WinEnumerationError {
    fn from(value: CfgMgrError) -> Self {
        Self::CfgMgr32Error(value)
    }
}

#[derive(Debug)]
pub struct CfgMgrError(pub u32);
impl error::Error for CfgMgrError {}
impl From<u32> for CfgMgrError {
    fn from(value: u32) -> Self {
        Self(value)
    }
}
impl Display for CfgMgrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => write!(f, "CR_SUCCESS"),
            1 => write!(f, "CR_DEFAULT"),
            2 => write!(f, "CR_OUT_OF_MEMORY"),
            3 => write!(f, "CR_INVALID_POINTER"),
            4 => write!(f, "CR_INVALID_FLAG"),
            5 => write!(f, "CR_INVALID_DEVNODE/INST"),
            6 => write!(f, "CR_INVALID_RES_DES"),
            7 => write!(f, "CR_INVALID_LOG_CONF"),
            8 => write!(f, "CR_INVALID_ARBITRATOR"),
            9 => write!(f, "CR_INVALID_NODELIST"),
            10 => write!(f, "CR_DEVNODE/INST_HAS_REQS"),
            11 => write!(f, "CR_INVALID_RESOURCEID"),
            12 => write!(f, "CR_DLVXD_NOT_FOUND"),
            13 => write!(f, "CR_NO_SUCH_DEVNODE/INST"),
            14 => write!(f, "CR_NO_MORE_LOG_CONF"),
            15 => write!(f, "CR_NO_MORE_RES_DES"),
            16 => write!(f, "CR_ALREADY_SUCH_DEVNODE/INST"),
            17 => write!(f, "CR_INVALID_RANGE_LIST"),
            18 => write!(f, "CR_INVALID_RANGE"),
            19 => write!(f, "CR_FAILURE"),
            20 => write!(f, "CR_NO_SUCH_LOGICAL_DEV"),
            21 => write!(f, "CR_CREATE_BLOCKED"),
            22 => write!(f, "CR_NOT_SYSTEM_VM"),
            23 => write!(f, "CR_REMOVE_VETOED"),
            24 => write!(f, "CR_APM_VETOED"),
            25 => write!(f, "CR_INVALID_LOAD_TYPE"),
            26 => write!(f, "CR_BUFFER_SMALL"),
            27 => write!(f, "CR_NO_ARBITRATOR"),
            28 => write!(f, "CR_NO_REGISTRY_HANDLE"),
            29 => write!(f, "CR_REGISTRY_ERROR"),
            30 => write!(f, "CR_INVALID_DEVICE_ID"),
            31 => write!(f, "CR_INVALID_DATA"),
            32 => write!(f, "CR_INVALID_API"),
            33 => write!(f, "CR_DEVLOADER_NOT_READY"),
            34 => write!(f, "CR_NEED_RESTART"),
            35 => write!(f, "CR_NO_MORE_HW_PROFILES"),
            36 => write!(f, "CR_DEVICE_NOT_THERE"),
            37 => write!(f, "CR_NO_SUCH_VALUE"),
            38 => write!(f, "CR_WRONG_TYPE"),
            39 => write!(f, "CR_INVALID_PRIORITY"),
            40 => write!(f, "CR_NOT_DISABLEABLE"),
            41 => write!(f, "CR_FREE_RESOURCES"),
            42 => write!(f, "CR_QUERY_VETOED"),
            43 => write!(f, "CR_CANT_SHARE_IRQ"),
            44 => write!(f, "CR_NO_DEPENDENT"),
            45 => write!(f, "CR_SAME_RESOURCES"),
            46 => write!(f, "CR_NO_SUCH_REGISTRY_KEY"),
            47 => write!(f, "CR_INVALID_MACHINENAME"),
            48 => write!(f, "CR_REMOTE_COMM_FAILURE"),
            49 => write!(f, "CR_MACHINE_UNAVAILABLE"),
            50 => write!(f, "CR_NO_CM_SERVICES"),
            51 => write!(f, "CR_ACCESS_DENIED"),
            52 => write!(f, "CR_CALL_NOT_IMPLEMENTED"),
            53 => write!(f, "CR_INVALID_PROPERTY"),
            54 => write!(f, "CR_DEVICE_INTERFACE_ACTIVE"),
            55 => write!(f, "CR_NO_SUCH_DEVICE_INTERFACE"),
            56 => write!(f, "CR_INVALID_REFERENCE_STRING"),
            57 => write!(f, "CR_INVALID_CONFLICT_LIST"),
            58 => write!(f, "CR_INVALID_INDEX"),
            59 => write!(f, "CR_INVALID_STRUCTURE_SIZE"),
            _ => write!(f, "unknown error {}", self.0),
        }
    }
}

fn try_parse_guid(inp: &NullU16) -> Option<GUID> {
    let inp = OsString::from_wide(inp.as_ref());
    let inp = inp.to_string_lossy();
    let mut inp = inp.as_ref();

    // Expect {stuff}
    if let Some(x) = inp.strip_prefix("{") {
        inp = x;
    }
    if let Some(x) = inp.strip_suffix("\0") {
        inp = x;
    }
    if let Some(x) = inp.strip_suffix("}") {
        inp = x;
    }

    let mut parts = inp.splitn(5, "-");

    // The first three parts are little/native-endian
    let a = u32::from_str_radix(parts.next()?, 16).ok()?;
    let b = u16::from_str_radix(parts.next()?, 16).ok()?;
    let c = u16::from_str_radix(parts.next()?, 16).ok()?;
    // This is special, always big-endian
    let d = u16::from_str_radix(parts.next()?, 16).ok()?;

    let mut e = [0; 8];
    e[0] = (d >> 8) as u8;
    e[1] = d as u8;
    let e_part = parts.next()?;
    if e_part.len() != 12 {
        return None;
    }
    // And then this trailer is just a list of bytes
    for i in 0..6 {
        e[2 + i] = u8::from_str_radix(&e_part[i * 2..(i + 1) * 2], 16).ok()?;
    }

    Some(GUID {
        data1: a,
        data2: b,
        data3: c,
        data4: e,
    })
}

fn multi_sz_to_list(inp: &[u16]) -> Vec<NullU16> {
    let mut ret = inp
        .split(|x| *x == 0)
        .map(|x| NullU16::from(x))
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
    dev_inst_id: &NullU16,
) -> Result<Vec<NullU16>, CfgMgrError> {
    loop {
        let mut list_sz = 0;
        let ret = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut list_sz,
                guid,
                dev_inst_id.as_ref().as_ptr(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if ret != CR_SUCCESS {
            return Err(CfgMgrError::from(ret).into());
        }

        let mut buf = vec![0; list_sz as usize];
        let ret = unsafe {
            CM_Get_Device_Interface_ListW(
                guid,
                dev_inst_id.as_ref().as_ptr(),
                buf.as_mut_ptr(),
                list_sz,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if ret == CR_BUFFER_SMALL {
            continue;
        }
        if ret != CR_SUCCESS {
            return Err(CfgMgrError::from(ret).into());
        }

        return Ok(multi_sz_to_list(&buf));
    }
}

fn get_devnode_str_property(
    devnode: u32,
    key: &DEVPROPKEY,
    key_err_name: &'static str,
) -> Result<NullU16, WinEnumerationError> {
    let mut buf_sz = 0;
    let mut ptype = 0;
    let ret = unsafe {
        CM_Get_DevNode_PropertyW(devnode, key, &mut ptype, ptr::null_mut(), &mut buf_sz, 0)
    };
    if ret != CR_BUFFER_SMALL {
        return Err(CfgMgrError::from(ret).into());
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
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
        return Err(CfgMgrError::from(ret).into());
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
    }

    unsafe {
        buf.set_len(buf_sz as usize / 2);
    }

    Ok(NullU16::from(buf))
}

fn get_devnode_custom_str_property(
    devnode: u32,
    key: windows_strings::PCWSTR,
    key_err_name: &'static str,
) -> Result<NullU16, WinEnumerationError> {
    let mut buf_sz = 0;
    let mut rtype = 0;
    let ret = unsafe {
        CM_Get_DevNode_Custom_PropertyW(
            devnode,
            key.as_ptr(),
            &mut rtype,
            ptr::null_mut(),
            &mut buf_sz,
            0,
        )
    };
    if ret != CR_BUFFER_SMALL {
        return Err(CfgMgrError::from(ret).into());
    }
    if rtype != REG_SZ {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
    }

    let mut buf = vec![0u16; (buf_sz as usize + 1) / 2];
    let ret = unsafe {
        CM_Get_DevNode_Custom_PropertyW(
            devnode,
            key.as_ptr(),
            &mut rtype,
            buf.as_mut_ptr() as *mut c_void,
            &mut buf_sz,
            0,
        )
    };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }
    if rtype != REG_SZ {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
    }

    unsafe {
        buf.set_len(buf_sz as usize / 2);
    }

    Ok(NullU16::from(buf))
}

fn get_devnode_custom_multi_str_property(
    devnode: u32,
    key: windows_strings::PCWSTR,
    key_err_name: &'static str,
) -> Result<Vec<NullU16>, WinEnumerationError> {
    let mut buf_sz = 0;
    let mut rtype = 0;
    let ret = unsafe {
        CM_Get_DevNode_Custom_PropertyW(
            devnode,
            key.as_ptr(),
            &mut rtype,
            ptr::null_mut(),
            &mut buf_sz,
            0,
        )
    };
    if ret != CR_BUFFER_SMALL {
        return Err(CfgMgrError::from(ret).into());
    }
    if rtype != REG_MULTI_SZ {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
    }

    let mut buf = vec![0u16; (buf_sz as usize + 1) / 2];
    let ret = unsafe {
        CM_Get_DevNode_Custom_PropertyW(
            devnode,
            key.as_ptr(),
            &mut rtype,
            buf.as_mut_ptr() as *mut c_void,
            &mut buf_sz,
            0,
        )
    };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }
    if rtype != REG_MULTI_SZ {
        return Err(WinEnumerationError::UnexpectedPropertyType(key_err_name));
    }

    unsafe {
        buf.set_len(buf_sz as usize / 2);
    }

    Ok(multi_sz_to_list(&buf))
}

fn get_dev_inst_id(instance_path: &[u16]) -> Result<Vec<u16>, WinEnumerationError> {
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
    if ret != CR_BUFFER_SMALL {
        return Err(CfgMgrError::from(ret).into());
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(WinEnumerationError::UnexpectedPropertyType(
            "DEVPKEY_Device_InstanceId",
        ));
    }

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
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }
    if ptype != DEVPROP_TYPE_STRING {
        return Err(WinEnumerationError::UnexpectedPropertyType(
            "DEVPKEY_Device_InstanceId",
        ));
    }

    unsafe {
        buf.set_len(buf_sz as usize / 2);
    }

    Ok(buf)
}

fn probe_new_device(guid: GUID, instance_path: &[u16]) -> Result<(), WinEnumerationError> {
    // At the beginning here, we have an instance path (starts with \\?\) to _something_,
    // but we don't know anything else about it

    // The first thing we want to do is map this to a device _instance_ ID (starts with USB\)
    let dev_inst_id = get_dev_inst_id(instance_path)?;
    dbg!(crate::DbgU16(&dev_inst_id));

    // Now we need to turn this into a devnode, which we can _actually_ use to do stuff with
    let mut devnode = 0;
    let ret =
        unsafe { CM_Locate_DevNodeW(&mut devnode, dev_inst_id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }
    dbg!(devnode);

    // Now we check if this is actually a WinUSB device
    let driver =
        get_devnode_str_property(devnode, &DEVPKEY_Device_Service, "DEVPKEY_Device_Service")?;
    dbg!(&driver);

    let driver = OsString::from_wide(driver.as_ref());
    if !driver.eq_ignore_ascii_case("WinUSB\0") {
        return Ok(());
    }
    log::debug!(
        "Probing {:?} which is a WinUSB instance",
        crate::DbgU16(instance_path)
    );

    // We *definitely* have a WinUSB instance now, but we have no idea if we're looking at a "desired"
    // "device interface class" or not.
    //
    // Possible "useless" classes include GUID_DEVINTERFACE_USB_HUB (which can't be used with WinUSB at all
    // or the (undocumented-ish) GUID_DEVINTERFACE_WINUSB_WINRT (which has weird extra restrictions)
    //
    // In order to check if this is one of the "actual" WinUSB GUIDs (and not e.g. the WinRT one),
    // we have to use this semi-undocumented query of the registry to see what the allowed GUIDs are.

    let mut possible_guids = Vec::new();

    if let Ok(dev_iface_guids) = get_devnode_custom_multi_str_property(
        devnode,
        windows_strings::w!("DeviceInterfaceGUIDs"),
        "DeviceInterfaceGUIDs",
    ) {
        for guid in dev_iface_guids {
            if let Some(guid) = try_parse_guid(&guid) {
                possible_guids.push(UnfuckedGUID(guid));
            }
        }
    }
    if let Ok(dev_iface_guid) = get_devnode_custom_str_property(
        devnode,
        windows_strings::w!("DeviceInterfaceGUID"),
        "DeviceInterfaceGUID",
    ) {
        if let Some(guid) = try_parse_guid(&dev_iface_guid) {
            possible_guids.push(UnfuckedGUID(guid));
        }
    }

    for x in &possible_guids {
        log::debug!("Registered WinUSB GUID: {:?}", x);
    }

    if !possible_guids.contains(&UnfuckedGUID(guid)) {
        return Ok(());
    }

    log::debug!("Probing further into {:?}", crate::DbgU16(instance_path));

    // Now we want to check the parent device, to see if we're part of a composite device.
    // The _ultimate_ goal is to find the USB hub device that owns us

    let mut parent = 0;
    let ret = unsafe { CM_Get_Parent(&mut parent, devnode, 0) };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }
    dbg!(parent);

    let parent_driver =
        get_devnode_str_property(parent, &DEVPKEY_Device_Service, "DEVPKEY_Device_Service")
            .unwrap();
    dbg!(&parent_driver);

    let parent_driver = OsString::from_wide(parent_driver.as_ref());

    let is_composite;
    let device_devnode;
    let hub_devnode;
    if parent_driver.eq_ignore_ascii_case("usbccgp\0")
        // some workaround for some Samsung devices??
        || parent_driver.eq_ignore_ascii_case("dg_ssudbus\0")
    {
        is_composite = true;
        device_devnode = parent;

        // Look up one level _again_, and that should be the hub
        let mut devnode = 0;
        let ret = unsafe { CM_Get_Parent(&mut devnode, parent, 0) };
        if ret != CR_SUCCESS {
            return Err(CfgMgrError::from(ret).into());
        }
        hub_devnode = devnode;
    } else {
        is_composite = false;
        device_devnode = devnode;

        // Assume/hope that the hub devnode is the parent devnode
        hub_devnode = parent;
    }

    // Find the "location info" and parse it for the hub port
    // This is apparently the only way to do it, the "Address" is apparently wrong
    // https://github.com/dorssel/usbipd-win/issues/82
    let loc_info = get_devnode_str_property(
        device_devnode,
        &DEVPKEY_Device_LocationInfo,
        "DEVPKEY_Device_LocationInfo",
    )?;
    log::debug!(
        "Device {:?} is located at {:?} on the hub",
        crate::DbgU16(instance_path),
        loc_info
    );
    let loc_info = OsString::from_wide(loc_info.as_ref()).to_ascii_lowercase();
    let loc_info = loc_info.to_string_lossy();
    let hub_port;
    if let Some(loc_info) = loc_info.strip_prefix("port_#") {
        if let Some((port, _)) = loc_info.split_once('.') {
            if let Ok(x) = u32::from_str_radix(port, 10) {
                hub_port = x;
            } else {
                return Err(WinEnumerationError::CouldNotFindHub);
            }
        } else {
            return Err(WinEnumerationError::CouldNotFindHub);
        }
    } else {
        return Err(WinEnumerationError::CouldNotFindHub);
    }
    log::debug!(
        "Device {:?} is located at port {} on the hub",
        crate::DbgU16(instance_path),
        hub_port
    );

    // Need to turn this hub devnode *back* into an instance id
    dbg!(hub_devnode);
    let hub_inst_id = get_devnode_str_property(
        hub_devnode,
        &DEVPKEY_Device_InstanceId,
        "DEVPKEY_Device_InstanceId",
    )?;
    log::debug!(
        "Hoping that {:?} is the hub for {:?}",
        hub_inst_id,
        crate::DbgU16(instance_path)
    );

    // And finally get an instance _path_ that refers to the hub
    let hub_paths = find_instance_paths(&GUID_DEVINTERFACE_USB_HUB, &hub_inst_id)?;
    if hub_paths.len() < 1 {
        return Err(WinEnumerationError::CouldNotFindHub);
    }
    if hub_paths.len() > 1 {
        log::warn!("More than one USB hub path found??");
    }
    let hub_path = &hub_paths[0];
    log::debug!(
        "Hoping that {:?} is the hub for {:?}",
        hub_path,
        crate::DbgU16(instance_path)
    );

    let hub_hfile = unsafe {
        CreateFileW(
            hub_path.as_ref().as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    dbg!(hub_hfile);

    unsafe {
        CloseHandle(hub_hfile);
    }

    // //

    // let mut child = 0;
    // let ret = unsafe { CM_Get_Child(&mut child, devnode, 0) };
    // dbg!(ret, child);

    // let driver = get_devnode_str_property(child, &DEVPKEY_Device_Service).unwrap();
    // dbg!(OsString::from_wide(&driver));

    Ok(())
}

unsafe extern "system" fn notify_cb(
    _hnotify: HCMNOTIFICATION,
    ctx: *const std::ffi::c_void,
    action: i32,
    event_data: *const CM_NOTIFY_EVENT_DATA,
    event_data_sz: u32,
) -> u32 {
    // NOTE: This function executes on a thread pool thread and not the main thread
    // We supposedly don't race here, see comment in Drop for WinNotificationHandler
    let (tx, hevent) = unsafe {
        let tx = &((*(ctx as *const WinNotificationHandler)).tx);
        let hevent = (*(ctx as *const WinNotificationHandler)).h_event;
        (tx.clone(), hevent)
    };

    let (guid, instance_path) = unsafe {
        if (*event_data).FilterType != CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE {
            log::warn!("Somehow got a bad CfgMgr32 notification type???");
            return ERROR_SUCCESS;
        }

        let guid = (*event_data).u.DeviceInterface.ClassGuid;
        let iface = &(*event_data).u.DeviceInterface;
        let link_sz = event_data_sz as usize
            - mem::offset_of!(CM_NOTIFY_EVENT_DATA, u.DeviceInterface.SymbolicLink);
        let link_ptr = iface.SymbolicLink.as_ptr();
        (guid, std::slice::from_raw_parts(link_ptr, link_sz / 2))
    };

    dbg!(action);
    dbg!(crate::DbgU16(instance_path));

    if action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL {
        let ret = probe_new_device(guid, instance_path);
        if let Err(e) = ret {
            log::warn!("Enumerating device failed! {}", e);
        }
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
            Flags: CM_NOTIFY_FILTER_FLAG_ALL_INTERFACE_CLASSES,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            Reserved: 0,
            u: CM_NOTIFY_FILTER_0 {
                DeviceInterface: CM_NOTIFY_FILTER_0_0 {
                    ClassGuid: GUID {
                        data1: 0,
                        data2: 0,
                        data3: 0,
                        data4: [0; 8],
                    },
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
            // Allegedly, according to https://community.osr.com/t/umdf-2-0-driver-registering-for-device-arrival-removal/48379/6
            // > CM_Unregister_Notification will make sure all the pending notifications are completed.
            CM_Unregister_Notification(self.h_notif);
            CloseHandle(self.h_event);
        }
    }
}
