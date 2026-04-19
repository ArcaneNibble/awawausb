//! Linux USB device enumeration helpers and completion handling
//!
//! The rest of the Linux support is strewn throughout `main.rs`
//! and should probably eventually get refactored.

use std::alloc::Layout;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::ptr;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

/// Helper function to read a sysfs file, because `dirfd` is not available in stable Rust
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

// The following are FFI declarations corresponding to
// linux/include/uapi/linux/usbdevice_fs.h

pub const USBDEVFS_CAP_NO_PACKET_SIZE_LIM: u32 = 0x04;
pub const USBDEVFS_CAP_REAP_AFTER_DISCONNECT: u32 = 0x10;

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

/// A [usbdevfs_urb] with trailing [usbdevfs_iso_packet_desc]s as a DST
#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct usbdevfs_urb_with_iso {
    pub urb: usbdevfs_urb,
    pub iso_frame_desc: [usbdevfs_iso_packet_desc],
}
impl usbdevfs_urb_with_iso {
    pub fn new(num_pkts: usize) -> Box<Self> {
        let urb_layout = Layout::new::<usbdevfs_urb>();
        let frames_layout = Layout::array::<usbdevfs_iso_packet_desc>(num_pkts).unwrap();
        let (final_layout, _) = urb_layout.extend(frames_layout).unwrap();
        let final_layout = final_layout.pad_to_align();

        unsafe {
            let mem = std::alloc::alloc_zeroed(final_layout);
            #[repr(C)]
            struct FatPointer {
                ptr: *mut u8,
                sz: usize,
            }
            let mem: *mut Self = std::mem::transmute(FatPointer {
                ptr: mem,
                sz: num_pkts,
            });
            Box::from_raw(mem)
        }
    }
}

/// Deal with `char` having different signed-ness on different CPUs
pub fn unfuck_bytes(x: &[libc::c_char]) -> &[u8] {
    unsafe { &*(x as *const [libc::c_char] as *const [u8]) }
}

/// A wrapper around a sysfs fd and a `/dev/bus/usb` device fd
#[derive(Debug)]
pub struct LinuxHandles {
    pub sysfs_fd: i32,
    pub dev_fd: i32,
    /// A pointer back into the [USBStubEngine](crate::USBStubEngine),
    /// which is used to control the size of the buffer that is needed
    /// by the `kqueue` event loop.
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

    /// When changing configurations, redetect what the kernel thinks about interfaces
    ///
    /// This exists for two reasons:
    /// 1. In case the kernel and us disagree about the interpretation of (likely malformed) USB descriptors
    /// 2. In case a driver changes the active alternate setting on an interface.
    ///    (It is not clear in the WebUSB spec how this ought to be handled…)
    ///
    /// If this is ever removed, we would no longer need to hang on to a sysfs dirfd handle.
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
                let dirname = CStr::from_bytes_until_nul(unfuck_bytes(&dirent.d_name)).unwrap();
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

    /// Check kernel capabilities, in case they're _way_ too old
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

/// A USB transfer which is currently in-flight
///
/// This wraps (and owns) both the URB submitted to the kernel,
/// as well as the user data buffer.
///
/// ```text
/// USBDevice._linux_timeout_urb  ------------+
///                                           |
/// kernel  -------------------------------+  |
///                                        |  |
///                  +----------+          |  |
///                  v          |          v  v
/// +-- LinuxURBWrapper --+     |    +-- usbdevfs_urb --+     +-- Vec<u8> payload --+
/// | urb  ---------------------+--> | buffer  -------------> | <raw data>          |
/// | buf  ------------------+  +----- usercontext      |     +---------------------+
/// | ...                 |  |       | ...              |       ^
/// +---------------------+  |       +------------------+       |
///                          +----------------------------------+
/// ```
///
/// Note the mutually-self-referential links between [LinuxURBWrapper] and [usbdevfs_urb].
/// The kernel gives us back a pointer to `usbdevfs_urb`,
/// which we use to extract the (raw) `usercontext` pointer to the `LinuxURBWrapper`,
/// which we then convert back into an (owning) `Box`.
///
/// As a _huge_ hack, each USB device can have a single URB in flight which contains a timeout.
/// A pointer to the appropriate (kernel) URB is stored in the
/// [_linux_timeout_urb](crate::USBDevice::_linux_timeout_urb) field.
/// This works because timeouts are only used during the setup/enumeration process
/// on the web extension side (so no pages can have access to the device yet,
/// and this part of the process is serialized).
/// This will need to be redesigned if timeouts get used more generally.
#[derive(Debug)]
pub struct LinuxURBWrapper {
    pub txn_id: String,
    pub dir: crate::USBTransferDirection,
    pub buf: Vec<u8>,
    pub urb: Box<usbdevfs_urb>,
    /// If a (internal-use-only) timeout exists, this contains a timerfd.
    pub _timeout_fd: i32,
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
                txn_id: self.txn_id.clone(),
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
                txn_id: self.txn_id.clone(),
                babble,
                data,
                bytes_written: self.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else {
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: self.txn_id.clone(),
                error: crate::protocol::Errors::TransferError,
                bytes_written: self.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        }
    }
}
impl Drop for LinuxURBWrapper {
    fn drop(&mut self) {
        if self._timeout_fd >= 0 {
            unsafe {
                libc::close(self._timeout_fd);
            }
        }
    }
}

/// A USB isochronous transfer which is currently in-flight
///
/// This is almost identical to [LinuxURBWrapper] except that
/// the kernel URB object is followed (in-line in memory) by
/// a list of isochronous packet descriptors.
#[derive(Debug)]
pub struct LinuxIsoURBWrapper {
    pub txn_id: String,
    pub dir: crate::USBTransferDirection,
    pub buf: Vec<u8>,
    pub urb: Box<usbdevfs_urb_with_iso>,
}
impl LinuxIsoURBWrapper {
    pub fn notify_completion(self) {
        let mut had_unwanted_error = false;

        let num_pkts = self.urb.iso_frame_desc.len();
        let mut pkt_status = Vec::with_capacity(num_pkts);
        let mut pkt_lens = Vec::with_capacity(num_pkts);
        for i in 0..num_pkts {
            pkt_status.push(if self.urb.iso_frame_desc[i].status == 0 {
                crate::protocol::IsocPacketState::Ok
            } else if self.urb.iso_frame_desc[i].status == (-libc::EOVERFLOW) as u32 {
                crate::protocol::IsocPacketState::Babble
            } else {
                had_unwanted_error = true;
                crate::protocol::IsocPacketState::Error
            });
            pkt_lens.push(self.urb.iso_frame_desc[i].actual_length as u32);
        }
        let data = if self.dir == crate::USBTransferDirection::DeviceToHost {
            Some(URL_SAFE_NO_PAD.encode(&self.buf))
        } else {
            None
        };

        log::debug!(
            "isoc request {} finished, status {}, buf {:02x?} status {:08x?} len {:?}",
            self.txn_id,
            self.urb.urb.status,
            self.buf,
            pkt_status,
            pkt_lens,
        );

        // Send notification
        if had_unwanted_error
            || (self.urb.urb.status != 0 && self.urb.urb.status != -libc::EOVERFLOW)
        {
            // An error, of a type we don't "tolerate"
            let notif = crate::protocol::ResponseMessage::RequestError {
                txn_id: self.txn_id,
                error: crate::protocol::Errors::TransferError,
                bytes_written: self.urb.urb.actual_length as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else {
            // Success
            let notif = crate::protocol::ResponseMessage::IsocRequestComplete {
                txn_id: self.txn_id,
                data,
                pkt_status,
                pkt_len: pkt_lens,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        }
    }
}
