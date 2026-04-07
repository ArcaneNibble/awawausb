use std::ptr;

use core_foundation::{
    base::{CFAllocatorRef, CFTypeRef},
    dictionary::{CFDictionaryRef, CFMutableDictionaryRef},
    string::CFStringRef,
};
use libc::{kern_return_t, mach_port_t, size_t};

#[repr(C, align(8))]
pub struct OpaqueMachMessage {
    _data: [u8; 4096],
}
impl Default for OpaqueMachMessage {
    fn default() -> Self {
        Self { _data: [0; 4096] }
    }
}

pub const MACH_RCV_MSG: libc::c_int = 0x0000_0002;
pub const MACH_RCV_INVALID_NAME: kern_return_t = 0x10004002;

#[allow(non_upper_case_globals)]
pub const kIOUSBPipeStalled: kern_return_t = 0xe000404fu32 as i32;
#[allow(non_upper_case_globals)]
pub const kIOReturnExclusiveAccess: kern_return_t = 0xe00002c5u32 as i32;
#[allow(non_upper_case_globals)]
pub const kIOReturnOverrun: kern_return_t = 0xe00002e8u32 as i32;

#[repr(C)]
pub struct IONotificationPort {
    _private: [u8; 0],
}

#[repr(C)]
pub struct CFUUID {
    _private: [u8; 0],
}

#[repr(C)]
pub struct CFUUIDBytes {
    _b0: u8,
    _b1: u8,
    _b2: u8,
    _b3: u8,
    _b4: u8,
    _b5: u8,
    _b6: u8,
    _b7: u8,
    _b8: u8,
    _b9: u8,
    _b10: u8,
    _b11: u8,
    _b12: u8,
    _b13: u8,
    _b14: u8,
    _b15: u8,
}
impl std::fmt::Debug for CFUUIDBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self._b0,
            self._b1,
            self._b2,
            self._b3,
            self._b4,
            self._b5,
            self._b6,
            self._b7,
            self._b8,
            self._b9,
            self._b10,
            self._b11,
            self._b12,
            self._b13,
            self._b14,
            self._b15,
        )
    }
}
impl CFUUIDBytes {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self {
            _b0: bytes[0],
            _b1: bytes[1],
            _b2: bytes[2],
            _b3: bytes[3],
            _b4: bytes[4],
            _b5: bytes[5],
            _b6: bytes[6],
            _b7: bytes[7],
            _b8: bytes[8],
            _b9: bytes[9],
            _b10: bytes[10],
            _b11: bytes[11],
            _b12: bytes[12],
            _b13: bytes[13],
            _b14: bytes[14],
            _b15: bytes[15],
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(transparent)]
pub struct io_object_t(pub mach_port_t);
impl From<io_object_t> for mach_port_t {
    fn from(value: io_object_t) -> Self {
        value.0
    }
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBDevRequest {
    pub bmRequestType: u8,
    pub bRequest: u8,
    pub wValue: u16,
    pub wIndex: u16,
    pub wLength: u16,
    pub pData: *mut (),
    pub wLenDone: u32,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBDevRequestTO {
    pub bmRequestType: u8,
    pub bRequest: u8,
    pub wValue: u16,
    pub wIndex: u16,
    pub wLength: u16,
    pub pData: *mut (),
    pub wLenDone: u32,
    pub noDataTimeout: u32,
    pub completionTimeout: u32,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBFindInterfaceRequest {
    pub bInterfaceClass: u16,
    pub bInterfaceSubClass: u16,
    pub bInterfaceProtocol: u16,
    pub bAlternateSetting: u16,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBIsocFrame {
    pub frStatus: kern_return_t,
    pub frReqCount: u16,
    pub frActCount: u16,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBLowLatencyIsocFrame {
    pub frStatus: kern_return_t,
    pub frReqCount: u16,
    pub frActCount: u16,
    pub frTimeStamp: u64,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IUnknown {
    _reserved: *const (),
    pub QueryInterface: unsafe extern "C" fn(*const (), CFUUIDBytes, *mut *const ()) -> i32,
    pub AddRef: unsafe extern "C" fn(*const ()) -> u32,
    pub Release: unsafe extern "C" fn(*const ()) -> u32,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBDeviceStruct320 {
    pub IUnknown: IUnknown,
    pub CreateDeviceAsyncEventSource:
        unsafe extern "C" fn(*const (), *mut *const ()) -> kern_return_t,
    pub GetDeviceAsyncEventSource: unsafe extern "C" fn(*const ()) -> *const (),
    pub CreateDeviceAsyncPort: unsafe extern "C" fn(*const (), *mut mach_port_t) -> kern_return_t,
    pub GetDeviceAsyncPort: unsafe extern "C" fn(*const ()) -> mach_port_t,
    pub USBDeviceOpen: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub USBDeviceClose: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub GetDeviceClass: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetDeviceSubClass: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetDeviceProtocol: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetDeviceVendor: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceProduct: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceReleaseNumber: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceAddress: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceBusPowerAvailable: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub GetDeviceSpeed: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetNumberOfConfigurations: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetLocationID: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub GetConfigurationDescriptorPtr:
        unsafe extern "C" fn(*const (), u8, *mut *const ()) -> kern_return_t,
    pub GetConfiguration: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub SetConfiguration: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub GetBusFrameNumber: unsafe extern "C" fn(*const (), *mut u64, *mut u64) -> kern_return_t,
    pub ResetDevice: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub DeviceRequest: unsafe extern "C" fn(*const (), *mut IOUSBDevRequest) -> kern_return_t,
    pub DeviceRequestAsync: unsafe extern "C" fn(
        *const (),
        *const IOUSBDevRequest,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub CreateInterfaceIterator: unsafe extern "C" fn(
        *const (),
        *const IOUSBFindInterfaceRequest,
        *mut io_object_t,
    ) -> kern_return_t,
    pub USBDeviceOpenSeize: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub DeviceRequestTO: unsafe extern "C" fn(*const (), *mut IOUSBDevRequestTO) -> kern_return_t,
    pub DeviceRequestAsyncTO: unsafe extern "C" fn(
        *const (),
        *const IOUSBDevRequestTO,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub USBDeviceSuspend: unsafe extern "C" fn(*const (), bool) -> kern_return_t,
    pub USBDeviceAbortPipeZero: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub USBGetManufacturerStringIndex: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub USBGetProductStringIndex: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub USBGetSerialNumberStringIndex: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub USBDeviceReEnumerate: unsafe extern "C" fn(*const (), u32) -> kern_return_t,
    pub GetBusMicroFrameNumber:
        unsafe extern "C" fn(*const (), *mut u64, *mut u64) -> kern_return_t,
    pub GetIOUSBLibVersion: unsafe extern "C" fn(*const (), *mut u32, *mut u32) -> kern_return_t,
    pub GetBusFrameNumberWithTime:
        unsafe extern "C" fn(*const (), *mut u64, *mut u64) -> kern_return_t,
    pub GetUSBDeviceInformation: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub RequestExtraPower: unsafe extern "C" fn(*const (), u32, u32, *mut u32) -> kern_return_t,
    pub ReturnExtraPower: unsafe extern "C" fn(*const (), u32, u32) -> kern_return_t,
    pub GetExtraPowerAllocated: unsafe extern "C" fn(*const (), u32, *mut u32) -> kern_return_t,
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct IOUSBInterfaceStruct197 {
    pub IUnknown: IUnknown,
    pub CreateInterfaceAsyncEventSource:
        unsafe extern "C" fn(*const (), *mut *const ()) -> kern_return_t,
    pub GetInterfaceAsyncEventSource: unsafe extern "C" fn(*const ()) -> *const (),
    pub CreateInterfaceAsyncPort:
        unsafe extern "C" fn(*const (), *mut mach_port_t) -> kern_return_t,
    pub GetInterfaceAsyncPort: unsafe extern "C" fn(*const ()) -> mach_port_t,
    pub USBInterfaceOpen: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub USBInterfaceClose: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub GetInterfaceClass: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetInterfaceSubClass: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetInterfaceProtocol: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetDeviceVendor: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceProduct: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetDeviceReleaseNumber: unsafe extern "C" fn(*const (), *mut u16) -> kern_return_t,
    pub GetConfigurationValue: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetInterfaceNumber: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetAlternateSetting: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetNumEndpoints: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub GetLocationID: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub GetDevice: unsafe extern "C" fn(*const (), *mut io_object_t) -> kern_return_t,
    pub SetAlternateInterface: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub GetBusFrameNumber: unsafe extern "C" fn(*const (), *mut u64, *mut u64) -> kern_return_t,
    pub ControlRequest: unsafe extern "C" fn(*const (), u8, *mut IOUSBDevRequest) -> kern_return_t,
    pub ControlRequestAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *const IOUSBDevRequest,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub GetPipeProperties: unsafe extern "C" fn(
        *const (),
        u8,
        *mut u8,
        *mut u8,
        *mut u8,
        *mut u16,
        *mut u8,
    ) -> kern_return_t,
    pub GetPipeStatus: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub AbortPipe: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub ResetPipe: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub ClearPipeStall: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub ReadPipe: unsafe extern "C" fn(*const (), u8, *mut (), *mut u32) -> kern_return_t,
    pub WritePipe: unsafe extern "C" fn(*const (), u8, *const (), u32) -> kern_return_t,
    pub ReadPipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *mut (),
        u32,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub WritePipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *const (),
        u32,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub ReadIsochPipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *mut (),
        u64,
        u32,
        *mut IOUSBIsocFrame,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub WriteIsochPipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *const (),
        u64,
        u32,
        *mut IOUSBIsocFrame,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub ControlRequestTO:
        unsafe extern "C" fn(*const (), u8, *mut IOUSBDevRequestTO) -> kern_return_t,
    pub ControlRequestAsyncTO: unsafe extern "C" fn(
        *const (),
        u8,
        *const IOUSBDevRequestTO,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub ReadPipeTO:
        unsafe extern "C" fn(*const (), u8, *mut (), *mut u32, u32, u32) -> kern_return_t,
    pub WritePipeTO: unsafe extern "C" fn(*const (), u8, *const (), u32, u32, u32) -> kern_return_t,
    pub ReadPipeAsyncTO: unsafe extern "C" fn(
        *const (),
        u8,
        *mut (),
        u32,
        u32,
        u32,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub WritePipeAsyncTO: unsafe extern "C" fn(
        *const (),
        u8,
        *const (),
        u32,
        u32,
        u32,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub USBInterfaceGetStringIndex: unsafe extern "C" fn(*const (), *mut u8) -> kern_return_t,
    pub USBInterfaceOpenSeize: unsafe extern "C" fn(*const ()) -> kern_return_t,
    pub ClearPipeStallBothEnds: unsafe extern "C" fn(*const (), u8) -> kern_return_t,
    pub SetPipePolicy: unsafe extern "C" fn(*const (), u8, u16, u8) -> kern_return_t,
    pub GetBandwidthAvailable: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub GetEndpointProperties:
        unsafe extern "C" fn(*const (), u8, u8, u8, *mut u8, *mut u16, *mut u8) -> kern_return_t,
    pub LowLatencyReadIsochPipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *mut (),
        u64,
        u32,
        u32,
        *mut IOUSBLowLatencyIsocFrame,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub LowLatencyWriteIsochPipeAsync: unsafe extern "C" fn(
        *const (),
        u8,
        *const (),
        u64,
        u32,
        u32,
        *mut IOUSBLowLatencyIsocFrame,
        extern "C" fn(*const (), kern_return_t, *const ()),
        *const (),
    ) -> kern_return_t,
    pub LowLatencyCreateBuffer:
        unsafe extern "C" fn(*const (), *mut *mut (), size_t, u32) -> kern_return_t,
    pub LowLatencyDestroyBuffer: unsafe extern "C" fn(*const (), *mut ()) -> kern_return_t,
    pub GetBusMicroFrameNumber:
        unsafe extern "C" fn(*const (), *mut u64, *mut u64) -> kern_return_t,
    pub GetFrameListTime: unsafe extern "C" fn(*const (), *mut u32) -> kern_return_t,
    pub GetIOUSBLibVersion: unsafe extern "C" fn(*const (), *mut u32, *mut u32) -> kern_return_t,
}

#[allow(non_snake_case)]
pub fn kIOCFPlugInInterfaceID() -> *const CFUUID {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            ptr::null(),
            0xC2,
            0x44,
            0xE8,
            0x58,
            0x10,
            0x9C,
            0x11,
            0xD4,
            0x91,
            0xD4,
            0x00,
            0x50,
            0xE4,
            0xC6,
            0x42,
            0x6F,
        )
    }
}

#[allow(non_snake_case)]
pub fn kIOUSBDeviceUserClientTypeID() -> *const CFUUID {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            ptr::null(),
            0x9d,
            0xc7,
            0xb7,
            0x80,
            0x9e,
            0xc0,
            0x11,
            0xD4,
            0xa5,
            0x4f,
            0x00,
            0x0a,
            0x27,
            0x05,
            0x28,
            0x61,
        )
    }
}

#[allow(non_snake_case)]
pub fn kIOUSBInterfaceUserClientTypeID() -> *const CFUUID {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            ptr::null(),
            0x2d,
            0x97,
            0x86,
            0xc6,
            0x9e,
            0xf3,
            0x11,
            0xD4,
            0xad,
            0x51,
            0x00,
            0x0a,
            0x27,
            0x05,
            0x28,
            0x61,
        )
    }
}

#[allow(non_upper_case_globals)]
pub const kIOUSBDeviceInterfaceID320: CFUUIDBytes = CFUUIDBytes::new([
    0x01, 0xA2, 0xD0, 0xE9, 0x42, 0xF6, 0x4A, 0x87, 0x8B, 0x8B, 0x77, 0x05, 0x7C, 0x8C, 0xE0, 0xCE,
]);

#[allow(non_upper_case_globals)]
pub const kIOUSBInterfaceInterfaceID197: CFUUIDBytes = CFUUIDBytes::new([
    0xC6, 0x3D, 0x3C, 0x92, 0x08, 0x84, 0x11, 0xD7, 0x96, 0x92, 0x00, 0x03, 0x93, 0x3E, 0x3E, 0x3E,
]);

unsafe extern "C" {
    pub fn mach_msg(
        msg: *mut OpaqueMachMessage,
        option: libc::c_int,
        send_size: libc::c_uint,
        rcv_size: libc::c_uint,
        rcv_name: mach_port_t,
        timeout: libc::c_uint,
        notify: mach_port_t,
    ) -> kern_return_t;

    pub fn IONotificationPortCreate(main_port: mach_port_t) -> *mut IONotificationPort;
    pub fn IONotificationPortDestroy(notify: *mut IONotificationPort);
    pub fn IONotificationPortGetMachPort(notify: *mut IONotificationPort) -> mach_port_t;
    pub fn IODispatchCalloutFromMessage(
        _unused: *const (),
        msg: *const OpaqueMachMessage,
        reference: *const (),
    );

    pub fn IOServiceMatching(name: *const u8) -> CFMutableDictionaryRef;
    pub fn IOServiceAddMatchingNotification(
        notify: *mut IONotificationPort,
        notification_type: *const u8,
        matching: CFDictionaryRef,
        callback: extern "C" fn(*const (), io_object_t),
        refcon: *const (),
        notification: *mut io_object_t,
    ) -> kern_return_t;

    pub fn IOIteratorNext(iterator: io_object_t) -> io_object_t;
    pub fn IOObjectRelease(object: io_object_t) -> kern_return_t;

    pub fn IORegistryEntryCreateCFProperty(
        entry: io_object_t,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;

    pub fn IOCreatePlugInInterfaceForService(
        service: io_object_t,
        pluginType: *const CFUUID,
        interfaceType: *const CFUUID,
        theInterface: *mut *const (),
        theScore: *mut i32,
    ) -> kern_return_t;

    pub fn CFUUIDGetConstantUUIDWithBytes(
        _allocator: *const (),
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
        b: u8,
    ) -> *const CFUUID;
}
