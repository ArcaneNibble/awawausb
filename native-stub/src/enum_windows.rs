use std::alloc::Layout;
use std::collections::{HashMap, hash_map};
use std::error;
use std::ffi::{OsString, c_void};
use std::fmt::{Debug, Display};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::{Mutex, mpsc};

use usb_ch9::USBDescriptor;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Properties::*;
use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Registry::{REG_MULTI_SZ, REG_SZ};
use windows_sys::Win32::System::Threading::*;
use windows_sys::core::*;

use crate::{NullU16, OsStringFromWideWithNull};

const BOS_DESCRIPTOR_TYPE: u8 = 15;

#[repr(C, packed(1))]
#[allow(non_snake_case)]
struct USB_DESCRIPTOR_REQUEST_WORKS {
    ConnectionIndex: u32,
    bmRequest: u8,
    bRequest: u8,
    wValue: u16,
    wIndex: u16,
    wLength: u16,
    data: [u8],
}

#[repr(C, packed(1))]
#[allow(non_snake_case)]
struct USB_DESCRIPTOR_REQUEST_INITIAL {
    ConnectionIndex: u32,
    bmRequest: u8,
    bRequest: u8,
    wValue: u16,
    wIndex: u16,
    wLength: u16,
    data: [u8; 4],
}

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
    IoError(io::Error),
    UnexpectedPropertyType(&'static str),
    CouldNotFindHub,
    CouldNotFindInterfaceNo,
    CouldNotFindWinUSBGUID,
    DescriptorParsingProblem(&'static str),
}
impl error::Error for WinEnumerationError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match self {
            Self::CfgMgr32Error(e) => Some(e),
            Self::IoError(e) => Some(e),
            _ => None,
        }
    }
}
impl Display for WinEnumerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CfgMgr32Error(e) => write!(f, "CfgMgr32 returned {}", e),
            Self::IoError(e) => write!(f, "I/O error {}", e),
            Self::UnexpectedPropertyType(msg) => {
                write!(f, "tried to read {} but it had the wrong type", msg)
            }
            Self::CouldNotFindHub => write!(f, "Could not find the hub owning the device"),
            Self::CouldNotFindInterfaceNo => write!(f, "Could not figure out the interface number"),
            Self::CouldNotFindWinUSBGUID => {
                write!(f, "Couldn't find a working WinUSB GUID (initial probe)")
            }
            Self::DescriptorParsingProblem(msg) => {
                write!(f, "Could not deal with device's descriptors: {}", msg)
            }
        }
    }
}
impl From<CfgMgrError> for WinEnumerationError {
    fn from(value: CfgMgrError) -> Self {
        Self::CfgMgr32Error(value)
    }
}
impl From<io::Error> for WinEnumerationError {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
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

fn find_instance_paths(guid: &GUID, dev_inst_id: &NullU16) -> Result<Vec<NullU16>, CfgMgrError> {
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

fn get_dev_inst_id(instance_path: &[u16]) -> Result<NullU16, WinEnumerationError> {
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

    Ok(NullU16::from(buf))
}

struct HubHandle(HANDLE, u32);
impl HubHandle {
    fn get_descriptor(
        &self,
        desc_ty: u8,
        desc_idx: u8,
        w_index: u16,
    ) -> Result<Option<Vec<u8>>, io::Error> {
        let is_reading_long_desc = (desc_ty == usb_ch9::ch9_core::descriptor_types::CONFIGURATION)
            || (desc_ty == BOS_DESCRIPTOR_TYPE);

        // Do an initial read to see how big the descriptor is
        let mut rbytes = 0;
        let mut initial_desc = USB_DESCRIPTOR_REQUEST_INITIAL {
            ConnectionIndex: self.1,
            bmRequest: 0,
            bRequest: 0,
            wValue: ((desc_ty as u16) << 8) | (desc_idx as u16),
            wIndex: w_index,
            wLength: if is_reading_long_desc { 4 } else { 2 },
            data: [0; 4],
        };

        let ret = unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION,
                &mut initial_desc as *mut _ as *mut c_void,
                mem::size_of::<USB_DESCRIPTOR_REQUEST_INITIAL>() as u32,
                &mut initial_desc as *mut _ as *mut c_void,
                mem::size_of::<USB_DESCRIPTOR_REQUEST_INITIAL>() as u32,
                &mut rbytes,
                ptr::null_mut(),
            )
        };
        if ret == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error().unwrap() as u32 == ERROR_GEN_FAILURE {
                // This seems to be the error returned for invalid descriptor fetches
                return Ok(None);
            }
            return Err(err);
        }

        // Check bDescriptorType against what we asked for
        if initial_desc.data[1] != desc_ty {
            return Ok(None);
        }

        // Get the actual length to read
        let actual_len = if is_reading_long_desc {
            // wTotalLength
            (initial_desc.data[2] as usize) | ((initial_desc.data[3] as usize) << 8)
        } else {
            // bLength
            initial_desc.data[0] as usize
        };

        // Now allocate a buffer of the appropriate size
        let ref_layout = Layout::new::<USB_DESCRIPTOR_REQUEST_INITIAL>();
        let wanted_layout =
            Layout::from_size_align(ref_layout.size() - 4 + actual_len, ref_layout.align())
                .unwrap();
        let buf = unsafe {
            let buf = std::alloc::alloc_zeroed(wanted_layout);

            #[repr(C)]
            struct FatPointer {
                ptr: *mut u8,
                sz: usize,
            }

            mem::transmute::<_, &mut USB_DESCRIPTOR_REQUEST_WORKS>(FatPointer {
                ptr: buf,
                sz: actual_len,
            })
        };
        buf.ConnectionIndex = self.1;
        buf.wValue = ((desc_ty as u16) << 8) | (desc_idx as u16);
        buf.wIndex = w_index;
        buf.wLength = actual_len as u16;

        // Issue the _real_ request now
        let ret = unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION,
                buf as *mut _ as *mut c_void,
                mem::size_of_val(buf) as u32,
                buf as *mut _ as *mut c_void,
                mem::size_of_val(buf) as u32,
                &mut rbytes,
                ptr::null_mut(),
            )
        };
        if ret == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Some(buf.data.to_owned()))
    }
}
impl Drop for HubHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn probe_new_device(
    db: &Mutex<WinHotplugDatabase>,
    guid: GUID,
    instance_path: &[u16],
) -> Result<Option<WinHotplugNotification>, WinEnumerationError> {
    // At the beginning here, we have an instance path (starts with \\?\) to _something_,
    // but we don't know anything else about it
    log::debug!(
        "Enumeration asked to look into {:?}",
        crate::DbgU16(instance_path)
    );

    // The first thing we want to do is map this to a device _instance_ ID (starts with USB\...)
    let this_dev_inst_id = get_dev_inst_id(instance_path)?;
    log::debug!(
        "The instance ID for {:?} is {:?}",
        crate::DbgU16(instance_path),
        this_dev_inst_id
    );

    // Now we need to turn this into a devnode (a u32), which we can _actually_ use to query CfgMgr32
    let mut devnode = 0;
    let ret = unsafe {
        CM_Locate_DevNodeW(
            &mut devnode,
            this_dev_inst_id.as_ref().as_ptr(),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }

    // Now we check if this is actually a WinUSB device
    let driver =
        get_devnode_str_property(devnode, &DEVPKEY_Device_Service, "DEVPKEY_Device_Service")?;

    let driver = OsString::from_wide(driver.as_ref());
    if !driver.eq_ignore_ascii_case("WinUSB\0") {
        return Ok(None);
    }
    log::debug!(
        "Probing {:?} which is a WinUSB instance",
        crate::DbgU16(instance_path)
    );

    // We *definitely* have a WinUSB device now, but we have no idea if we're looking at a "desired"
    // "device interface class" or not.
    //
    // Possible "useless" classes include GUID_DEVINTERFACE_USB_HUB (which can't be used with WinUSB at all)
    // or the (undocumented-ish) GUID_DEVINTERFACE_WINUSB_WINRT (which has weird extra restrictions)
    //
    // In order to check if this is one of the "actual" WinUSB GUIDs (and not e.g. the WinRT one),
    // we have to use this semi-undocumented query of the registry to see what the allowed GUIDs are.
    // Then we check if we are one of them. If not, we bail out. This can potentially result in
    // multiple probes of the same WinUSB interface with multiple different GUIDs.
    // We deal with that later.

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
        log::debug!("This interface isn't a useful WinUSB GUID");
        return Ok(None);
    }

    probe_winusb_device_further(db, devnode, this_dev_inst_id, instance_path)
}

fn probe_winusb_device_further(
    db: &Mutex<WinHotplugDatabase>,
    devnode: u32,
    this_dev_inst_id: NullU16,
    instance_path: &[u16],
) -> Result<Option<WinHotplugNotification>, WinEnumerationError> {
    log::debug!("Probing further into {:?}", crate::DbgU16(instance_path));

    // Now we want to check the parent device, to see if we're part of a composite device.
    // The _ultimate_ goal is to find the USB hub device that owns us

    let mut parent = 0;
    let ret = unsafe { CM_Get_Parent(&mut parent, devnode, 0) };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }

    let parent_driver =
        get_devnode_str_property(parent, &DEVPKEY_Device_Service, "DEVPKEY_Device_Service")?;
    let parent_driver = OsString::from_wide(parent_driver.as_ref());

    let is_composite;
    let interface_no;
    let device_devnode;
    let hub_devnode;
    if parent_driver.eq_ignore_ascii_case("usbccgp\0")
        // some workaround for some Samsung devices??
        || parent_driver.eq_ignore_ascii_case("dg_ssudbus\0")
    {
        is_composite = true;
        device_devnode = parent;

        // Try to parse the device interface number (MI_xx)
        let inst_id_str = OsString::from_wide(this_dev_inst_id.as_ref()).to_ascii_lowercase();
        let inst_id_str = inst_id_str.to_string_lossy();
        if let Some(mut x) = inst_id_str.strip_prefix("usb\\vid_") {
            if x.len() > 4 {
                // Strip 4 hex digits after VID_
                x = &x[4..];
                if let Some(mut x) = x.strip_prefix("&pid_") {
                    if x.len() > 4 {
                        // Strip 4 hex digits after PID_
                        x = &x[4..];
                        if let Some(x) = x.strip_prefix("&mi_") {
                            if x.len() > 2 {
                                let iface_no = &x[..2];
                                interface_no = u8::from_str_radix(iface_no, 16)
                                    .map_err(|_| WinEnumerationError::CouldNotFindInterfaceNo)?;
                            } else {
                                return Err(WinEnumerationError::CouldNotFindInterfaceNo);
                            }
                        } else {
                            return Err(WinEnumerationError::CouldNotFindInterfaceNo);
                        }
                    } else {
                        return Err(WinEnumerationError::CouldNotFindInterfaceNo);
                    }
                } else {
                    return Err(WinEnumerationError::CouldNotFindInterfaceNo);
                }
            } else {
                return Err(WinEnumerationError::CouldNotFindInterfaceNo);
            }
        } else {
            return Err(WinEnumerationError::CouldNotFindInterfaceNo);
        }
        log::debug!(
            "{:?} should be bInterfaceNumber {:02x}",
            this_dev_inst_id,
            interface_no
        );

        // Look up one level _again_, and that should be the hub
        let mut devnode = 0;
        let ret = unsafe { CM_Get_Parent(&mut devnode, parent, 0) };
        if ret != CR_SUCCESS {
            return Err(CfgMgrError::from(ret).into());
        }
        hub_devnode = devnode;
    } else {
        is_composite = false;
        interface_no = 0;
        device_devnode = devnode;

        // Assume/hope that the hub devnode is the parent devnode
        hub_devnode = parent;
    }

    // We want the instance ID (USB\...) of the whole device, for lookups
    let whole_device_inst_id = get_devnode_str_property(
        device_devnode,
        &DEVPKEY_Device_InstanceId,
        "DEVPKEY_Device_InstanceId",
    )?;
    log::debug!(
        "The instance ID for the whole device of {:?} is {:?}",
        crate::DbgU16(instance_path),
        whole_device_inst_id
    );

    // We have something _useful_ now, so try to get access to the db
    let mut db = db.lock().expect("Hotplug db mutex poisoned");
    let WinHotplugDatabase {
        ref mut next_session_id,
        devices: ref mut db_devices,
        ref mut path_to_device_map,
    } = *db;

    // Make a device exist
    let whole_dev_inst_id_osstring = OsString::from_wide_null(whole_device_inst_id.as_ref());
    let db_device = db_devices.entry(whole_dev_inst_id_osstring);
    let need_to_probe_device_harder = match db_device {
        hash_map::Entry::Occupied(_) => false,
        hash_map::Entry::Vacant(_) => true,
    };

    let device_info;
    if need_to_probe_device_harder {
        log::debug!(
            "This is apparently the first time we've seen {:?}, probing even harder",
            whole_device_inst_id
        );

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
        if hub_hfile == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let hub_handle = HubHandle(hub_hfile, hub_port);
        log::debug!(
            "Caching useful descriptors for this device, handle = {:?}",
            hub_hfile
        );

        // *sigh*, apparently this is how you get the current configuration
        // (at least it gives you the device descriptor for free?)
        let mut conn_info =
            unsafe { MaybeUninit::<USB_NODE_CONNECTION_INFORMATION_EX>::zeroed().assume_init() };
        conn_info.ConnectionIndex = hub_port;
        let mut rbytes = 0;
        let ret = unsafe {
            DeviceIoControl(
                hub_hfile,
                IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
                &mut conn_info as *mut _ as *mut c_void,
                mem::size_of::<USB_NODE_CONNECTION_INFORMATION_EX>() as u32,
                &mut conn_info as *mut _ as *mut c_void,
                mem::size_of::<USB_NODE_CONNECTION_INFORMATION_EX>() as u32,
                &mut rbytes,
                ptr::null_mut(),
            )
        };
        if ret == 0 {
            return Err(io::Error::last_os_error().into());
        }

        let dev_desc = unsafe {
            std::slice::from_raw_parts(
                &conn_info.DeviceDescriptor as *const _ as *const u8,
                mem::size_of::<USB_DEVICE_DESCRIPTOR>(),
            )
        };
        let dev_desc = usb_ch9::ch9_core::DeviceDescriptor::from_bytes(dev_desc)
            .ok_or(WinEnumerationError::DescriptorParsingProblem(
                "failed to parse device descriptor",
            ))?
            .0;

        // If a device is blocked, don't report it or otherwise continue
        if crate::blocklists::is_blocked_device(dev_desc.idVendor, dev_desc.idProduct) {
            log::info!(
                "Device {:04x}:{:04x} is blocked",
                { dev_desc.idVendor },  //
                { dev_desc.idProduct }  //
            );
            return Ok(None);
        }

        let mut config_descs = Vec::new();
        for cfg_i in 0..dev_desc.bNumConfigurations {
            let cfg_desc = hub_handle.get_descriptor(
                usb_ch9::ch9_core::descriptor_types::CONFIGURATION,
                cfg_i,
                0,
            )?;
            if let Some(cfg_desc) = cfg_desc {
                config_descs.push(cfg_desc);
            } else {
                log::warn!("failed to get configuration descriptor {:02x}", cfg_i)
            }
        }

        let mut string_table_cache = HashMap::new();
        let mut string_cache_lang = 0;
        let mut cache_string = |id: u8| -> Result<(), WinEnumerationError> {
            if let hash_map::Entry::Vacant(v) = string_table_cache.entry(id) {
                let string_desc = hub_handle.get_descriptor(
                    usb_ch9::ch9_core::descriptor_types::STRING,
                    id,
                    string_cache_lang,
                )?;
                if let Some(string_desc) = string_desc {
                    if id == 0 {
                        if let Some((lang_desc, _)) =
                            usb_ch9::ch9_core::StringDescriptor::from_bytes(&string_desc)
                        {
                            if lang_desc.bytes.len() >= 2 {
                                string_cache_lang =
                                    u16::from_le_bytes([lang_desc.bytes[0], lang_desc.bytes[1]]);
                                log::debug!(
                                    "USB descriptor language is 0x{:04x}",
                                    string_cache_lang
                                );
                            }
                        }
                    }

                    v.insert(string_desc);
                } else {
                    log::warn!("failed to get string descriptor {:02x}", id)
                }
            }

            Ok(())
        };
        cache_string(0)?;
        if dev_desc.iManufacturer != 0 {
            cache_string(dev_desc.iManufacturer)?;
        }
        if dev_desc.iProduct != 0 {
            cache_string(dev_desc.iProduct)?;
        }
        if dev_desc.iSerialNumber != 0 {
            cache_string(dev_desc.iSerialNumber)?;
        }
        for cfg_desc in &config_descs {
            for desc in usb_ch9::parse_descriptor_set(&cfg_desc) {
                match desc {
                    usb_ch9::DescriptorRef::Config(d) => {
                        if d.iConfiguration != 0 {
                            cache_string(d.iConfiguration)?;
                        }
                    }
                    usb_ch9::DescriptorRef::Interface(d) => {
                        if d.iInterface != 0 {
                            cache_string(d.iInterface)?;
                        }
                    }
                    _ => {}
                }
            }
        }

        let bos_desc = if dev_desc.bcdUSB >= 0x0201 {
            let bos_desc = hub_handle.get_descriptor(BOS_DESCRIPTOR_TYPE, 0, 0)?;
            if let Some(bos_desc) = bos_desc {
                Some(bos_desc)
            } else {
                log::warn!("failed to get BOS descriptor");
                None
            }
        } else {
            None
        };

        device_info = Some(WinHotplugDeviceInfo {
            dev_desc: *dev_desc,
            current_config: conn_info.CurrentConfigurationValue,
            config_descs,
            bos_desc,
            string_descs: string_table_cache,
        })
    } else {
        log::debug!("We've seen this device before, associating interface with it...");
        device_info = None;
    }

    // Now that we are _all_ probed out, we can insert information into the DB
    let db_device = db_device.or_insert_with(|| {
        let session_id = *next_session_id;
        *next_session_id = session_id + 1;
        DeviceState {
            session_id,
            interfaces: HashMap::new(),
        }
    });
    let this_if_path = OsString::from_wide_null(instance_path);
    db_device.interfaces.insert(interface_no, this_if_path);

    // Update the DB for unplug handling
    let this_if_path = OsString::from_wide_null(instance_path);
    let whole_dev_inst_id_osstring = OsString::from_wide_null(whole_device_inst_id.as_ref());
    let orig = path_to_device_map.insert(this_if_path, whole_dev_inst_id_osstring);
    if orig.is_some() {
        log::warn!(
            "Found interface path {:?} multiple times",
            crate::DbgU16(instance_path)
        );
    }

    Ok(Some(WinHotplugNotification::NewInterface {
        session_id: db_device.session_id,
        interface_no,
        interface_path: OsString::from_wide_null(instance_path),
        whole_device: !is_composite,
        device_info,
    }))
}

fn get_existing_usb_device_instances() -> Result<Vec<NullU16>, CfgMgrError> {
    loop {
        let mut list_sz = 0;
        let ret = unsafe {
            CM_Get_Device_ID_List_SizeW(
                &mut list_sz,
                windows_strings::w!("USB").as_ptr(),
                CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT,
            )
        };
        if ret != CR_SUCCESS {
            return Err(CfgMgrError::from(ret).into());
        }

        let mut buf = vec![0; list_sz as usize];
        let ret = unsafe {
            CM_Get_Device_ID_ListW(
                windows_strings::w!("USB").as_ptr(),
                buf.as_mut_ptr(),
                list_sz,
                CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT,
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

fn try_probe_existing_device(
    db: &Mutex<WinHotplugDatabase>,
    existing_dev_inst: NullU16,
) -> Result<Option<WinHotplugNotification>, WinEnumerationError> {
    // Turn this into a devnode (a u32), which we can _actually_ use to query CfgMgr32
    let mut devnode = 0;
    let ret = unsafe {
        CM_Locate_DevNodeW(
            &mut devnode,
            existing_dev_inst.as_ref().as_ptr(),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };
    if ret != CR_SUCCESS {
        return Err(CfgMgrError::from(ret).into());
    }

    // Now we check if this is actually a WinUSB device
    let driver =
        get_devnode_str_property(devnode, &DEVPKEY_Device_Service, "DEVPKEY_Device_Service")?;

    let driver = OsString::from_wide(driver.as_ref());
    if !driver.eq_ignore_ascii_case("WinUSB\0") {
        return Ok(None);
    }
    log::debug!(
        "Initial probing {:?} which is a WinUSB instance",
        existing_dev_inst
    );

    // We *definitely* have a WinUSB device now, so we need to find its device interface GUIDs.
    // We then pick the first one that works.

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

    // Try to find some instance path that works
    let mut instance_path = None;
    let mut instance_paths;
    for winusb_guid in possible_guids {
        instance_paths = find_instance_paths(&winusb_guid.0, &existing_dev_inst)?;
        if instance_paths.len() > 0 {
            instance_path = Some(&instance_paths[0]);
            break;
        }
    }
    if instance_path.is_none() {
        return Err(WinEnumerationError::CouldNotFindWinUSBGUID);
    }
    let instance_path = instance_path.unwrap();

    probe_winusb_device_further(db, devnode, existing_dev_inst, instance_path.as_ref())
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
    let (tx, hevent, db) = unsafe {
        let tx = &((*(ctx as *const WinNotificationHandler)).tx);
        let db = &((*(ctx as *const WinNotificationHandler)).database);
        let hevent = (*(ctx as *const WinNotificationHandler)).h_event;
        (tx.clone(), hevent, db)
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

    let notification;
    match action {
        CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL => {
            match probe_new_device(db, guid, instance_path) {
                Ok(Some(notif)) => notification = notif,
                Ok(None) => {
                    // The device was to be ignored
                    return ERROR_SUCCESS;
                }
                Err(e) => {
                    log::warn!("Enumerating device failed! {}", e);
                    return ERROR_SUCCESS;
                }
            }
        }
        CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL => {
            let mut db = db.lock().expect("Hotplug db mutex poisoned");
            let WinHotplugDatabase {
                next_session_id: _,
                devices: ref mut db_devices,
                ref mut path_to_device_map,
            } = *db;
            let this_if_path = OsString::from_wide_null(instance_path);
            if let hash_map::Entry::Occupied(path_to_dev_entry) =
                path_to_device_map.entry(this_if_path.clone())
            {
                log::debug!(
                    "Unplugging interface {:?} of device {:?}",
                    path_to_dev_entry.key(),
                    path_to_dev_entry.get()
                );

                if let hash_map::Entry::Occupied(mut dev_to_state_entry) =
                    db_devices.entry(path_to_dev_entry.get().clone())
                {
                    let mut iface = None;
                    for (iface_no, iface_path) in dev_to_state_entry.get().interfaces.iter() {
                        if *iface_path == this_if_path {
                            iface = Some(*iface_no);
                            break;
                        }
                    }
                    if let Some(iface) = iface {
                        dev_to_state_entry.get_mut().interfaces.remove(&iface);
                        notification = WinHotplugNotification::RemoveInterface {
                            session_id: dev_to_state_entry.get().session_id,
                            interface_no: iface,
                            interface_path: this_if_path,
                        };
                        // Also wipe out _our_ state if appropriate
                        if dev_to_state_entry.get().interfaces.len() == 0 {
                            log::debug!(
                                "Unplugging last interface of device {:?}",
                                path_to_dev_entry.get()
                            );
                            dev_to_state_entry.remove();
                        }
                    } else {
                        log::warn!("Unplugging an interface we don't have! {:?}", this_if_path);
                        return ERROR_SUCCESS;
                    }
                } else {
                    log::warn!(
                        "Unplugging a device we don't have! {:?}",
                        path_to_dev_entry.get()
                    );
                    return ERROR_SUCCESS;
                }

                path_to_dev_entry.remove();
            } else {
                // Don't care about this interface
                return ERROR_SUCCESS;
            }
        }
        _ => {
            log::warn!("Unknown CfgMgr32 notification action {} ???", action);
            return ERROR_SUCCESS;
        }
    }

    if let Ok(_) = tx.send(notification) {
        unsafe {
            SetEvent(hevent);
        }
    }

    ERROR_SUCCESS
}

#[derive(Debug)]
struct DeviceState {
    session_id: u64,
    /// Map from (primary) interface number (MI_xx) to device _path_
    interfaces: HashMap<u8, OsString>,
}

#[derive(Debug)]
struct WinHotplugDatabase {
    next_session_id: u64,
    /// Map from device instance ID (of the "whole" device) to state
    devices: HashMap<OsString, DeviceState>,
    /// Map from \\?\ paths to device instance IDs
    ///
    /// The point of this is to avoid flaky parsing, and this is used for
    /// the unplug pathway
    path_to_device_map: HashMap<OsString, OsString>,
}
impl WinHotplugDatabase {
    fn new() -> Self {
        Self {
            next_session_id: 0,
            devices: HashMap::new(),
            path_to_device_map: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct WinHotplugDeviceInfo {
    pub dev_desc: usb_ch9::ch9_core::DeviceDescriptor,
    pub current_config: u8,
    pub config_descs: Vec<Vec<u8>>,
    pub bos_desc: Option<Vec<u8>>,
    pub string_descs: HashMap<u8, Vec<u8>>,
}

#[derive(Debug)]
pub enum WinHotplugNotification {
    NewInterface {
        session_id: u64,
        interface_no: u8,
        interface_path: OsString,
        whole_device: bool,
        device_info: Option<WinHotplugDeviceInfo>,
    },
    RemoveInterface {
        session_id: u64,
        interface_no: u8,
        interface_path: OsString,
    },
}

#[derive(Debug)]
pub struct WinNotificationHandler {
    h_notif: HCMNOTIFICATION,
    pub h_event: HANDLE,
    database: Mutex<WinHotplugDatabase>,
    tx: mpsc::Sender<WinHotplugNotification>,
    rx: mpsc::Receiver<WinHotplugNotification>,
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
            ptr::addr_of_mut!((*self_).database).write(Mutex::new(WinHotplugDatabase::new()));
            ptr::addr_of_mut!((*self_).tx).write(tx);
            ptr::addr_of_mut!((*self_).rx).write(rx);
        }
    }

    pub fn probe_existing(&self) -> Result<Vec<WinHotplugNotification>, WinEnumerationError> {
        let existing_dev_insts = get_existing_usb_device_instances()?;
        let mut probe_results = Vec::new();

        for existing_dev_inst in existing_dev_insts {
            match try_probe_existing_device(&self.database, existing_dev_inst) {
                Ok(Some(probe)) => {
                    probe_results.push(probe);
                }
                Ok(None) => {}
                Err(err) => log::warn!("Initial probing device failed! {}", err),
            };
        }

        Ok(probe_results)
    }

    pub fn get_notif(&self) -> Option<WinHotplugNotification> {
        match self.rx.try_recv() {
            Ok(notif) => Some(notif),
            Err(_) => None,
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
