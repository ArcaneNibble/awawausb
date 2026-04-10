use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::ptr;
use std::rc::Rc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn read_entire_sysfs_file(dirfd: i32, filename: &CStr) -> io::Result<Option<Vec<u8>>> {
    let filefd =
        unsafe { libc::openat(dirfd, filename.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if filefd < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(err);
    }

    let mut rsfile = unsafe { File::from_raw_fd(filefd) };
    let mut ret = Vec::new();
    rsfile.read_to_end(&mut ret)?;
    Ok(Some(ret))
}

pub const USBDEVFS_CAP_NO_PACKET_SIZE_LIM: u32 = 0x04;
pub const USBDEVFS_CAP_REAP_AFTER_DISCONNECT: u32 = 0x10;

pub const USBDEVFS_DISCONNECT_CLAIM_IF_DRIVER: u32 = 0x01;
pub const USBDEVFS_DISCONNECT_CLAIM_EXCEPT_DRIVER: u32 = 0x02;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct usbdevfs_disconnect_claim {
    pub interface: u32,
    pub flags: u32,
    pub driver: [u8; 256],
}

pub const USBDEVFS_URB_ISO_ASAP: u32 = 0x02;
pub const USBDEVFS_URB_TYPE_ISO: u8 = 0;
pub const USBDEVFS_URB_TYPE_INTERRUPT: u8 = 1;
pub const USBDEVFS_URB_TYPE_CONTROL: u8 = 2;
pub const USBDEVFS_URB_TYPE_BULK: u8 = 3;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct usbdevfs_setinterface {
    pub interface: u32,
    pub altsetting: u32,
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct usbdevfs_iso_packet_desc {
    pub length: u32,
    pub actual_length: u32,
    pub status: u32,
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct usbdevfs_urb {
    pub type_: u8,
    pub endpoint: u8,
    pub status: i32,
    pub flags: u32,
    pub buffer: *mut (),
    pub buffer_length: i32,
    pub actual_length: i32,
    pub start_frame: i32,
    pub number_of_packets: i32,
    pub error_count: i32,
    pub signr: u32,
    pub usercontext: *mut (),
}

#[derive(Debug)]
pub struct LinuxHandles {
    pub sysfs_fd: i32,
    pub dev_fd: i32,
    pub(crate) decr_event_count: *const Cell<usize>,
}
impl LinuxHandles {
    pub fn new() -> Self {
        Self {
            sysfs_fd: -1,
            dev_fd: -1,
            decr_event_count: ptr::null(),
        }
    }

    pub fn reprobe_ifaces(&self) -> io::Result<HashMap<u8, crate::USBInterfaceState>> {
        let dir = unsafe { libc::fdopendir(libc::fcntl(self.sysfs_fd, libc::F_DUPFD_CLOEXEC, 0)) };
        if dir.is_null() {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            libc::rewinddir(dir);
        }

        let mut if_states = HashMap::new();

        loop {
            let dirent = unsafe { libc::readdir(dir) };
            if dirent.is_null() {
                break;
            }
            let dirent = unsafe { *dirent };

            if dirent.d_type == libc::DT_DIR {
                let dirname = CStr::from_bytes_until_nul(&dirent.d_name).unwrap();
                if dirname != c"." && dirname != c".." {
                    let mut iface_fn = dirname.to_bytes().to_vec();
                    iface_fn.extend_from_slice(b"/bInterfaceNumber\x00");
                    let iface = read_entire_sysfs_file(
                        self.sysfs_fd,
                        CString::from_vec_with_nul(iface_fn).unwrap().as_c_str(),
                    )?;
                    if iface.is_none() {
                        continue;
                    }
                    let iface = iface.unwrap();

                    let mut alt_setting_fn = dirname.to_bytes().to_vec();
                    alt_setting_fn.extend_from_slice(b"/bAlternateSetting\x00");
                    let alt_setting = read_entire_sysfs_file(
                        self.sysfs_fd,
                        CString::from_vec_with_nul(alt_setting_fn)
                            .unwrap()
                            .as_c_str(),
                    )?
                    .unwrap_or_default();

                    let iface = str::from_utf8(&iface).unwrap_or_default().trim();
                    let iface = u8::from_str_radix(iface, 16).unwrap_or_default();
                    let alt_setting = str::from_utf8(&alt_setting).unwrap_or_default().trim();
                    let alt_setting = u8::from_str_radix(alt_setting, 10).unwrap_or_default();

                    let old = if_states.insert(
                        iface,
                        crate::USBInterfaceState {
                            alt_setting,
                            claimed: false,
                        },
                    );
                    if old.is_some() {
                        log::warn!("Duplicate interface?? {}", iface);
                    }
                }
            }
        }

        unsafe { libc::closedir(dir) };

        Ok(if_states)
    }

    pub fn get_capabilities(&mut self) -> io::Result<u32> {
        let mut out = 0;
        unsafe {
            let ret = libc::ioctl(self.dev_fd, libc::_IOR::<u32>(b'U' as u32, 26), &mut out);
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(out)
    }
}
impl Drop for LinuxHandles {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.sysfs_fd);
            libc::close(self.dev_fd);
            if !self.decr_event_count.is_null() {
                // Decrement needed event count
                let needed_events = &*self.decr_event_count;
                needed_events.update(|x| x - 1);
            }
        }
    }
}

#[derive(Debug)]
pub struct LinuxURBWrapper {
    pub txn_id: String,
    pub dir: crate::USBTransferDirection,
    pub buf: Vec<u8>,
    pub urb: Box<usbdevfs_urb>,
    pub _handles_rc: Rc<RefCell<LinuxHandles>>,
}
impl LinuxURBWrapper {
    pub fn notify_completion(self) {
        log::debug!(
            "request {} finished, status {}, buf {:02x?}",
            self.txn_id,
            self.urb.status,
            self.buf,
        );

        // Send notification
        if self.urb.status == -libc::EPIPE {
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: self.txn_id,
                error: crate::protocol::Errors::Stall,
                bytes_written: self.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else if self.urb.status == 0 || self.urb.status == -libc::EOVERFLOW {
            let babble = self.urb.status == -libc::EOVERFLOW;
            let data = if self.dir == crate::USBTransferDirection::DeviceToHost {
                if self.urb.type_ == USBDEVFS_URB_TYPE_CONTROL {
                    Some(URL_SAFE_NO_PAD.encode(&self.buf[8..]))
                } else {
                    Some(URL_SAFE_NO_PAD.encode(&self.buf))
                }
            } else {
                None
            };
            let notif = crate::protocol::ResponseMessage::RequestComplete {
                txn_id: self.txn_id,
                babble,
                data,
                bytes_written: self.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else {
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: self.txn_id,
                error: crate::protocol::Errors::TransferError,
                bytes_written: self.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        }
    }
}
