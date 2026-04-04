use core_foundation::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use libc::{kern_return_t, mach_port_t};

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

#[repr(C)]
pub struct IONotificationPort {
    _private: [u8; 0],
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
}
