use std::cell::Cell;
use std::ptr;

use core_foundation::{
    base::{CFType, FromVoid, TCFType},
    number::CFNumber,
    string::CFString,
};
use libc::{kern_return_t, mach_port_t};

use super::macos_sys::*;

pub fn get_usb_cached_string(obj: io_object_t, s: &'static str) -> Option<String> {
    let string = unsafe {
        IORegistryEntryCreateCFProperty(
            obj,
            CFString::from_static_string(s).as_CFTypeRef() as *const _,
            ptr::null(),
            0,
        )
    };

    if !string.is_null() {
        let string = unsafe { CFType::from_void(string) };
        if let Some(string) = string.downcast::<CFString>() {
            return Some(string.to_string());
        }
    }
    None
}

pub fn get_max_pkt_0(obj: io_object_t) -> Option<u8> {
    let pktsz = unsafe {
        IORegistryEntryCreateCFProperty(
            obj,
            CFString::from_static_string("bMaxPacketSize0").as_CFTypeRef() as *const _,
            ptr::null(),
            0,
        )
    };

    if !pktsz.is_null() {
        let pktsz = unsafe { CFType::from_void(pktsz) };
        if let Some(pktsz) = pktsz.downcast::<CFNumber>() {
            if let Some(pktsz) = pktsz.to_i32() {
                return Some(pktsz as u8);
            }
        }
    }
    None
}

pub fn get_bcd_usb(obj: io_object_t) -> Option<u16> {
    let bcdusb = unsafe {
        IORegistryEntryCreateCFProperty(
            obj,
            CFString::from_static_string("bcdUSB").as_CFTypeRef() as *const _,
            ptr::null(),
            0,
        )
    };

    if !bcdusb.is_null() {
        let bcdusb = unsafe { CFType::from_void(bcdusb) };
        if let Some(bcdusb) = bcdusb.downcast::<CFNumber>() {
            if let Some(bcdusb) = bcdusb.to_i32() {
                return Some(bcdusb as u16);
            }
        }
    }
    None
}

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
pub struct IOUSBDeviceStruct(
    *mut *const IOUSBDeviceStruct320,
    pub(crate) *const Cell<usize>,
);
#[allow(non_snake_case)]
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

            Self(device as *mut *const IOUSBDeviceStruct320, ptr::null())
        }
    }

    pub fn CreateDeviceAsyncPort(&mut self) -> Result<mach_port_t, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).CreateDeviceAsyncPort)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn USBDeviceOpen(&mut self) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).USBDeviceOpen)(self.0 as *const ()) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
    pub fn USBDeviceClose(&mut self) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).USBDeviceClose)(self.0 as *const ()) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
    pub fn GetDeviceClass(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceClass)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetDeviceSubClass(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceSubClass)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetDeviceProtocol(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceProtocol)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetDeviceVendor(&mut self) -> Result<u16, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceVendor)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetDeviceProduct(&mut self) -> Result<u16, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceProduct)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetDeviceReleaseNumber(&mut self) -> Result<u16, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetDeviceReleaseNumber)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn USBGetManufacturerStringIndex(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret =
            unsafe { ((**self.0).USBGetManufacturerStringIndex)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn USBGetProductStringIndex(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).USBGetProductStringIndex)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn USBGetSerialNumberStringIndex(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret =
            unsafe { ((**self.0).USBGetSerialNumberStringIndex)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetNumberOfConfigurations(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetNumberOfConfigurations)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetConfigurationDescriptorPtr(&mut self, conf: u8) -> Result<*const (), kern_return_t> {
        let mut out = ptr::null();
        let ret = unsafe {
            ((**self.0).GetConfigurationDescriptorPtr)(self.0 as *const (), conf, &mut out)
        };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetConfiguration(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetConfiguration)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn SetConfiguration(&mut self, config: u8) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).SetConfiguration)(self.0 as *const (), config) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }

    pub fn CreateInterfaceIterator(
        &mut self,
        find: &IOUSBFindInterfaceRequest,
    ) -> Result<io_object_t, kern_return_t> {
        let mut out = io_object_t(0);
        let ret =
            unsafe { ((**self.0).CreateInterfaceIterator)(self.0 as *const (), find, &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }

    pub fn USBDeviceReEnumerate(&mut self, options: u32) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).USBDeviceReEnumerate)(self.0 as *const (), options) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }

    pub fn ctrl_xfer(
        &mut self,
        mut xfer_obj: crate::USBTransfer,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: u32,
    ) -> Result<(), kern_return_t> {
        // Prepare our object, which is a Box on the heap so that it doesn't move
        assert!(length as usize <= xfer_obj.buf.capacity());
        let buf_ptr = xfer_obj.buf.as_mut_ptr();
        let xfer_ptr = Box::into_raw(Box::new(xfer_obj));

        // Prepare OS transfer object
        let req = IOUSBDevRequestTO {
            bmRequestType: request_type,
            bRequest: request,
            wValue: value,
            wIndex: index,
            wLength: length,
            pData: buf_ptr as *mut (),
            wLenDone: 0,
            noDataTimeout: timeout as u32,
            completionTimeout: timeout as u32,
        };

        // Submit transfer
        let ret = unsafe {
            ((**self.0).DeviceRequestAsyncTO)(
                self.0 as *const (),
                &req,
                crate::USBStubEngine::iokit_usb_completion,
                xfer_ptr as *const (),
            )
        };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
}
impl Drop for IOUSBDeviceStruct {
    fn drop(&mut self) {
        unsafe {
            ((**self.0).IUnknown.Release)(self.0 as *const ());
            if !self.1.is_null() {
                // Decrement needed event count
                let needed_events = &*self.1;
                needed_events.update(|x| x - 1);
            }
        }
    }
}

#[derive(Debug)]
pub struct IOUSBInterfaceStruct(*mut *const IOUSBInterfaceStruct197);
#[allow(non_snake_case)]
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

    pub fn USBInterfaceOpen(&mut self) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).USBInterfaceOpen)(self.0 as *const ()) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
    pub fn USBInterfaceClose(&mut self) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).USBInterfaceClose)(self.0 as *const ()) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }

    pub fn GetInterfaceNumber(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetInterfaceNumber)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn GetAlternateSetting(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetAlternateSetting)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
}
impl Drop for IOUSBInterfaceStruct {
    fn drop(&mut self) {
        unsafe {
            ((**self.0).IUnknown.Release)(self.0 as *const ());
        }
    }
}
