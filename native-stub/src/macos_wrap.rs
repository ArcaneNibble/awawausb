use std::ptr;

use core_foundation::{
    base::{CFType, FromVoid, TCFType},
    number::CFNumber,
    string::CFString,
};

use super::macos_sys::*;

pub fn get_session_id(obj: io_object_t) -> Option<u64> {
    let sessionid = unsafe {
        IORegistryEntryCreateCFProperty(
            obj,
            CFString::from_static_string("sessionID").as_CFTypeRef() as *const _,
            ptr::null(),
            0,
        )
    };

    if !sessionid.is_null() {
        let sessionid = unsafe { CFType::from_void(sessionid) };
        if let Some(sessionid) = sessionid.downcast::<CFNumber>() {
            if let Some(sessionid) = sessionid.to_i64() {
                return Some(sessionid as u64);
            }
        }
    }
    None
}

#[derive(Debug)]
pub struct IOUSBDeviceStruct(*mut *const IOUSBDeviceStruct320);
impl IOUSBDeviceStruct {
    /// Turns an IOKit io_object_t into a USB device interface
    ///
    /// Takes ownership of and *releases* the object
    pub unsafe fn new(obj: io_object_t) -> Self {
        unsafe {
            // Get IOKit plugin interface
            // TODO: libusb has a loop workaround thing here. Do we actually need that?
            let mut iokit_plugin = ptr::null();
            let mut score = 0;
            let ret = IOCreatePlugInInterfaceForService(
                obj,
                kIOUSBDeviceUserClientTypeID(),
                kIOCFPlugInInterfaceID(),
                &mut iokit_plugin,
                &mut score,
            );
            assert_eq!(
                ret, 0,
                "IOCreatePlugInInterfaceForService failed! 0x{:08x}",
                ret
            );

            let plugin_iunk = *(iokit_plugin as *const *const IUnknown);
            let mut device = ptr::null();
            let ret = ((*plugin_iunk).QueryInterface)(
                iokit_plugin,
                kIOUSBDeviceInterfaceID320,
                &mut device,
            );
            assert!(ret >= 0, "QueryInterface failed!");
            assert!(!device.is_null(), "QueryInterface failed!");

            // Don't need the plugin interface anymore
            ((*plugin_iunk).Release)(iokit_plugin);
            IOObjectRelease(obj);

            Self(device as *mut *const IOUSBDeviceStruct320)
        }
    }

    pub fn test(&self) {
        let mut lib_ver = 0;
        let mut fam_ver = 0;
        let ret = unsafe {
            ((**self.0).GetIOUSBLibVersion)(self.0 as *const (), &mut lib_ver, &mut fam_ver)
        };
        assert_eq!(ret, 0);
        println!("{:08x} {:08x}", lib_ver, fam_ver);

        // iterator
        let iter_ifaces = IOUSBFindInterfaceRequest {
            bInterfaceClass: 0xffff,
            bInterfaceSubClass: 0xffff,
            bInterfaceProtocol: 0xffff,
            bAlternateSetting: 0xffff,
        };
        let mut iface_iter_obj = io_object_t(0);
        let ret = unsafe {
            ((**self.0).CreateInterfaceIterator)(
                self.0 as *const (),
                &iter_ifaces,
                &mut iface_iter_obj,
            )
        };
        assert_eq!(ret, 0);
        println!("iface iterator {:?}", iface_iter_obj);

        let mut iface_iokit;
        loop {
            iface_iokit = unsafe { IOIteratorNext(iface_iter_obj) };
            if iface_iokit.0 == 0 {
                break;
            }

            let iface = unsafe { IOUSBInterfaceStruct::new(iface_iokit) };

            let mut lib_ver = 0;
            let mut fam_ver = 0;
            let ret = unsafe {
                ((**iface.0).GetIOUSBLibVersion)(iface.0 as *const (), &mut lib_ver, &mut fam_ver)
            };
            assert_eq!(ret, 0);
            println!("{:08x} {:08x}", lib_ver, fam_ver);
        }
    }
}
impl Drop for IOUSBDeviceStruct {
    fn drop(&mut self) {
        unsafe {
            ((**self.0).IUnknown.Release)(self.0 as *const ());
        }
    }
}

#[derive(Debug)]
pub struct IOUSBInterfaceStruct(*mut *const IOUSBInterfaceStruct197);
impl IOUSBInterfaceStruct {
    /// Turns an IOKit io_object_t into a USB interface interface
    ///
    /// Takes ownership of and *releases* the object
    pub unsafe fn new(obj: io_object_t) -> Self {
        unsafe {
            // Get IOKit plugin interface
            let mut iokit_plugin = ptr::null();
            let mut score = 0;
            let ret = IOCreatePlugInInterfaceForService(
                obj,
                kIOUSBInterfaceUserClientTypeID(),
                kIOCFPlugInInterfaceID(),
                &mut iokit_plugin,
                &mut score,
            );
            assert_eq!(
                ret, 0,
                "IOCreatePlugInInterfaceForService failed! 0x{:08x}",
                ret
            );

            let plugin_iunk = *(iokit_plugin as *const *const IUnknown);
            let mut device = ptr::null();
            let ret = ((*plugin_iunk).QueryInterface)(
                iokit_plugin,
                kIOUSBInterfaceInterfaceID197,
                &mut device,
            );
            assert!(ret >= 0, "QueryInterface failed!");
            assert!(!device.is_null(), "QueryInterface failed!");

            // Don't need the plugin interface anymore
            ((*plugin_iunk).Release)(iokit_plugin);
            IOObjectRelease(obj);

            Self(device as *mut *const IOUSBInterfaceStruct197)
        }
    }
}
impl Drop for IOUSBInterfaceStruct {
    fn drop(&mut self) {
        unsafe {
            ((**self.0).IUnknown.Release)(self.0 as *const ());
        }
    }
}
