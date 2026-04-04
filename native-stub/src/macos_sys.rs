use core_foundation::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use libc::{kern_return_t, mach_port_t};

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
    pub fn IONotificationPortCreate(main_port: mach_port_t) -> *mut IONotificationPort;
    pub fn IONotificationPortDestroy(notify: *mut IONotificationPort);
    pub fn IONotificationPortGetMachPort(notify: *mut IONotificationPort) -> mach_port_t;

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
