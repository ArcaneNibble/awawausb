use libc::mach_port_t;

#[repr(C)]
pub struct IONotificationPort {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub fn IONotificationPortCreate(main_port: mach_port_t) -> *mut IONotificationPort;
    pub fn IONotificationPortDestroy(notify: *mut IONotificationPort);
    pub fn IONotificationPortGetMachPort(notify: *mut IONotificationPort) -> mach_port_t;
}
