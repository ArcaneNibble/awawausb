use std::cell::{Cell, RefCell};
use std::collections::{HashMap, hash_map};
use std::convert::Infallible;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString, OsStr};
use std::fmt::Debug;
use std::io;
use std::mem;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::ptr;
#[cfg(target_os = "macos")]
use std::rc::Rc;
#[cfg(target_os = "linux")]
use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(any(windows, target_os = "linux"))]
use usb_ch9::USBDescriptor;

pub mod protocol;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::*;
#[cfg(target_os = "linux")]
mod udev_sys;
#[cfg(target_os = "linux")]
use udev_sys::*;

#[cfg(target_os = "macos")]
use kqueue_sys::*;

#[cfg(target_os = "macos")]
mod macos_sys;
#[cfg(target_os = "macos")]
use macos_sys::*;
#[cfg(target_os = "macos")]
mod macos_wrap;
#[cfg(target_os = "macos")]
use macos_wrap::*;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod stdio_unix;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use stdio_unix::*;

#[cfg(windows)]
mod stdio_windows;
#[cfg(windows)]
use stdio_windows::*;
#[cfg(windows)]
mod enum_windows;
#[cfg(windows)]
use enum_windows::*;
#[cfg(windows)]
mod usb_windows;
#[cfg(windows)]
use usb_windows::*;

#[cfg(windows)]
use windows_sys::Win32::System::Threading::*;

#[cfg(windows)]
pub trait OsStringFromWideWithNull {
    fn from_wide_null(inp: &[u16]) -> OsString;
}
#[cfg(windows)]
impl OsStringFromWideWithNull for OsString {
    fn from_wide_null(mut inp: &[u16]) -> OsString {
        if inp.last() == Some(&0) {
            inp = &inp[..inp.len() - 1]
        }
        OsString::from_wide(inp)
    }
}
#[cfg(windows)]
/// An owning container of a definitely-null-terminated UCS-2 string
pub struct NullU16(Vec<u16>);
#[cfg(windows)]
impl From<&[u16]> for NullU16 {
    fn from(value: &[u16]) -> Self {
        let mut ret = value.to_owned();
        if ret.last() != Some(&0) {
            ret.push(0);
        }
        Self(ret)
    }
}
#[cfg(windows)]
impl From<Vec<u16>> for NullU16 {
    fn from(mut value: Vec<u16>) -> Self {
        if value.last() != Some(&0) {
            value.push(0);
        }
        Self(value)
    }
}
#[cfg(windows)]
impl From<NullU16> for Vec<u16> {
    fn from(value: NullU16) -> Self {
        value.0
    }
}
#[cfg(windows)]
impl AsRef<[u16]> for NullU16 {
    fn as_ref(&self) -> &[u16] {
        &self.0
    }
}
#[cfg(windows)]
impl AsMut<[u16]> for NullU16 {
    fn as_mut(&mut self) -> &mut [u16] {
        &mut self.0
    }
}
#[cfg(windows)]
impl Debug for NullU16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = OsString::from_wide(&self.0[..self.0.len() - 1]);
        str.fmt(f)
    }
}

#[cfg(windows)]
/// A non-owning debugging helper for a usually-null-terminated UCS-2 string
pub struct DbgU16<'a>(pub &'a [u16]);
#[cfg(windows)]
impl<'a> From<&'a [u16]> for DbgU16<'a> {
    fn from(value: &'a [u16]) -> Self {
        Self(value)
    }
}
#[cfg(windows)]
impl<'a> Debug for DbgU16<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = if self.0.last() == Some(&0) {
            OsString::from_wide(&self.0[..self.0.len() - 1])
        } else {
            OsString::from_wide(&self.0)
        };
        str.fmt(f)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum USBTransferDirection {
    HostToDevice,
    DeviceToHost,
}

/// State regarding each interface
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct USBInterfaceState {
    pub alt_setting: u8,
    pub claimed: bool,

    #[cfg(target_os = "macos")]
    _macos_iface_idx: usize,
    #[cfg(target_os = "macos")]
    /// For (reverse) mapping from endpoint address to pipeRef
    _macos_ep_addrs: Vec<u8>,
}

/// For the "new" API, whether operations on a device should send a completion *now* or not
pub enum DeviceOpResult {
    /// The request is already finished, and the framework should send an *empty* completion
    SendCompletionNow,
    /// The request is in progress, or otherwise the framework should *not* send anything
    ManualCompletion,
}
type DeviceResult = Result<DeviceOpResult, protocol::Errors>;

/// Handle for a USB device
///
/// May or may not be opened. Holds OS-specific handle,
/// as well as generic metadata (i.e. descriptors, strings).
#[derive(Debug)]
pub struct USBDevice {
    pub device_descriptor: usb_ch9::ch9_core::DeviceDescriptor,
    pub config_descriptors: Vec<Vec<u8>>,
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
    pub serial_number: Option<String>,

    pub reformatted_config_descriptors: Vec<protocol::DeviceConfiguration>,
    /// Map from (bConfigurationValue, bEndpointAddress) to bInterfaceNumber
    ///
    /// Different interfaces cannot use the same endpoints
    pub ep_to_idx: HashMap<(u8, u8), u8>,
    /// Map from bInterfaceNumber to bInterfaceNumber
    ///
    /// This is only actually used on Windows
    pub iad_map: HashMap<u8, u8>,

    pub opened: bool,
    pub current_configuration_id: u8,
    /// Map from bInterfaceNumber to state
    pub current_if_state: HashMap<u8, USBInterfaceState>,

    /// Map from bInterfaceNumber to *path*
    #[cfg(windows)]
    _win_iface_paths: HashMap<u8, OsString>,
    #[cfg(windows)]
    _win_iface_handles: HashMap<u8, WinUSBHandle>,
    // Windows caches these *before* we ever set up
    #[cfg(windows)]
    _win_string_cache: HashMap<u8, Vec<u8>>,
    #[cfg(windows)]
    _win_bos_desc: Option<Vec<u8>>,

    /// Linux-specific state
    #[cfg(target_os = "linux")]
    _linux_handles: LinuxHandles,
    #[cfg(target_os = "linux")]
    _linux_epoll_fd: i32,
    /// The (one at a time) outstanding URB with a timeout
    #[cfg(target_os = "linux")]
    _linux_timeout_urb: Option<*const usbdevfs_urb>,

    /// Map from endpoint address to (interface index, pipeRef)
    ///
    /// (macOS specific)
    #[cfg(target_os = "macos")]
    _macos_ep_to_idx: HashMap<u8, (usize, u8)>,
    #[cfg(target_os = "macos")]
    _macos_dev: IOUSBDeviceStruct,
    #[cfg(target_os = "macos")]
    _macos_ifaces: Vec<Rc<RefCell<IOUSBInterfaceStruct>>>,
}
impl USBDevice {
    /// Send a device plug-in notification, while also stashing ourselves into the engine
    pub fn send_plug_notification(self, sessionid: u64, engine: Pin<&USBStubEngine>) {
        let notif = protocol::ResponseMessage::NewDevice {
            sid: sessionid.to_string(),

            bcdUSB: self.device_descriptor.bcdUSB,
            bDeviceClass: self.device_descriptor.bDeviceClass,
            bDeviceSubClass: self.device_descriptor.bDeviceSubClass,
            bDeviceProtocol: self.device_descriptor.bDeviceProtocol,
            idVendor: self.device_descriptor.idVendor,
            idProduct: self.device_descriptor.idProduct,
            bcdDevice: self.device_descriptor.bcdDevice,
            manufacturer: self.vendor_name.clone(),
            product: self.product_name.clone(),
            serial: self.serial_number.clone(),

            current_config: self.current_configuration_id,
            configs: self.reformatted_config_descriptors.clone(),
        };

        let mut devices = engine.usb_devices.borrow_mut();
        let old_dev = devices.insert(sessionid, self);
        if old_dev.is_some() {
            log::warn!("Got a duplicate sessionID?? 0x{:x}", sessionid);
        }

        // Send notification
        let notif = serde_json::to_string(&notif).unwrap();
        write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    }

    pub(crate) fn reformat_config_descriptors(&mut self, create_iface_dummy_info: bool) {
        // FIXME: This is only stored for the active config
        let mut iad_map = HashMap::new();
        let mut ep_to_idx = HashMap::new();
        let mut configs = Vec::new();
        for cfg_desc in &self.config_descriptors {
            let mut this_config_desc = None;
            for desc in usb_ch9::parse_descriptor_set(&cfg_desc) {
                match desc {
                    usb_ch9::DescriptorRef::Config(d) => {
                        if this_config_desc.is_some() {
                            #[cfg(any(windows, target_os = "macos"))]
                            {
                                // On Windows and macOS, we get individual descriptors per config
                                log::warn!("Bogus extra configuration descriptor? {:?}", desc)
                            }
                            #[cfg(target_os = "linux")]
                            {
                                // On Linux, we just get everything concatenated,
                                // so start a new configuration now
                                configs.push(this_config_desc.take().unwrap());
                                this_config_desc = Some(protocol::DeviceConfiguration {
                                    bConfigurationValue: d.bConfigurationValue,
                                    iConfiguration: d.iConfiguration,
                                    interfaces: Vec::new(),
                                });
                            }
                        } else {
                            this_config_desc = Some(protocol::DeviceConfiguration {
                                bConfigurationValue: d.bConfigurationValue,
                                iConfiguration: d.iConfiguration,
                                interfaces: Vec::new(),
                            });
                        }
                    }
                    usb_ch9::DescriptorRef::Interface(d) => {
                        if let Some(this_config_desc) = &mut this_config_desc {
                            let current_alt_setting = if this_config_desc.bConfigurationValue
                                == self.current_configuration_id
                            {
                                let current_if_state =
                                    self.current_if_state.entry(d.bInterfaceNumber);
                                match current_if_state {
                                    hash_map::Entry::Occupied(e) => e.get().alt_setting,
                                    hash_map::Entry::Vacant(e) => {
                                        if create_iface_dummy_info {
                                            // On Windows, it's very annoying/difficult/impossible to get the real alt setting
                                            // (so we just... don't)
                                            e.insert(USBInterfaceState {
                                                alt_setting: 0,
                                                claimed: false,
                                                // These cannot occur, but must be present
                                                #[cfg(target_os = "macos")]
                                                _macos_iface_idx: 0,
                                                #[cfg(target_os = "macos")]
                                                _macos_ep_addrs: Vec::new(),
                                            });
                                        } else {
                                            log::warn!(
                                                "Descriptor has an interface 0x{:02x} we don't know about",
                                                d.bInterfaceNumber
                                            );
                                        }
                                        0
                                    }
                                }
                            } else {
                                // If the configuration isn't active, default the alt setting to 0
                                0
                            };

                            this_config_desc.interfaces.push(protocol::DeviceInterface {
                                bInterfaceNumber: d.bInterfaceNumber,
                                bAlternateSetting: d.bAlternateSetting,
                                bInterfaceClass: d.bInterfaceClass,
                                bInterfaceSubClass: d.bInterfaceSubClass,
                                bInterfaceProtocol: d.bInterfaceProtocol,
                                iInterface: d.iInterface,

                                current_alt_setting,
                                endpoints: Vec::new(),
                            });
                        } else {
                            log::warn!(
                                "Bogus interface descriptor without config descriptor? {:?}",
                                desc
                            )
                        }
                    }
                    usb_ch9::DescriptorRef::Endpoint(d) => {
                        if let Some(this_config_desc) = &mut this_config_desc {
                            if let Some(last_iface) = this_config_desc.interfaces.iter_mut().last()
                            {
                                let old_iface = ep_to_idx.insert(
                                    (this_config_desc.bConfigurationValue, d.bEndpointAddress),
                                    last_iface.bInterfaceNumber,
                                );
                                if let Some(old_iface) = old_iface {
                                    if old_iface != last_iface.bInterfaceNumber {
                                        log::warn!(
                                            "Endpoints incorrectly duplicated across interfaces? ep 0x{:02x} in iface 0x{:02x}",
                                            d.bEndpointAddress,
                                            last_iface.bInterfaceNumber
                                        );
                                    }
                                }

                                last_iface.endpoints.push(protocol::DeviceEndpoint {
                                    bEndpointAddress: d.bEndpointAddress,
                                    bmAttributes: d.bmAttributes,
                                    wMaxPacketSize: d.wMaxPacketSize,
                                });
                            } else {
                                log::warn!(
                                    "Bogus endpoint descriptor without interface descriptor? {:?}",
                                    desc
                                )
                            }
                        } else {
                            log::warn!(
                                "Bogus endpoint descriptor without config descriptor? {:?}",
                                desc
                            )
                        }
                    }
                    usb_ch9::DescriptorRef::IAD(d) => {
                        if let Some(this_config_desc) = &mut this_config_desc {
                            if this_config_desc.bConfigurationValue == self.current_configuration_id
                            {
                                for if_no in
                                    d.bFirstInterface..d.bFirstInterface + d.bInterfaceCount
                                {
                                    let orig = iad_map.insert(if_no, d.bFirstInterface);
                                    if orig.is_some() && orig != Some(d.bFirstInterface) {
                                        log::warn!("Overlapping IAD? {:?}", d)
                                    }
                                }
                            }
                        } else {
                            log::warn!(
                                "Bogus interface association descriptor without config descriptor? {:?}",
                                desc
                            )
                        }
                    }
                    _ => {}
                }
            }

            if this_config_desc.is_none() {
                log::warn!("Bogus configuration descriptor??");
                // Put a dummy value in
                this_config_desc = Some(protocol::DeviceConfiguration {
                    bConfigurationValue: 0,
                    iConfiguration: 0,
                    interfaces: Vec::new(),
                });
            }

            configs.push(this_config_desc.unwrap());
        }

        self.reformatted_config_descriptors = configs;
        self.ep_to_idx = ep_to_idx;
        self.iad_map = iad_map;
    }

    fn _check_open(&self) -> Result<(), protocol::Errors> {
        if !self.opened {
            Err(protocol::Errors::InvalidState)
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
impl USBDevice {
    pub fn setup(
        dev_usb_path: &Path,
        sysfs_dev_path: Option<&Path>,
        engine: Pin<&USBStubEngine>,
    ) -> io::Result<()> {
        let mut linux_dev = LinuxHandles::new();

        let dev_fd = unsafe {
            libc::open(
                CString::new(dev_usb_path.as_os_str().as_bytes())
                    .unwrap()
                    .as_ptr(),
                libc::O_RDWR | libc::O_CLOEXEC,
            )
        };
        if dev_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        linux_dev.dev_fd = dev_fd;

        let mut sysfs_char_path = PathBuf::new();
        let sysfs_dev_path = if let Some(p) = sysfs_dev_path {
            p
        } else {
            let char_dev_stat = unsafe {
                let mut stat = mem::MaybeUninit::<libc::stat>::zeroed();
                let ret = libc::fstat(dev_fd, stat.as_mut_ptr());
                if ret < 0 {
                    return Err(io::Error::last_os_error());
                }
                stat.assume_init()
            };
            // WTF is this?
            let maj = (((char_dev_stat.st_rdev & 0xFFFFF000_00000000) >> 32)
                | ((char_dev_stat.st_rdev & 0xFFF00) >> 8)) as u32;
            let min = (((char_dev_stat.st_rdev & 0xFFF_FFF0_0000) >> 12)
                | (char_dev_stat.st_rdev & 0xFF)) as u32;
            sysfs_char_path.push("/sys/dev/char");
            sysfs_char_path.push(format!("{}:{}", maj, min));
            &sysfs_char_path
        };

        let sysfs_dirfd = unsafe {
            libc::open(
                CString::new(sysfs_dev_path.as_os_str().as_bytes())
                    .unwrap()
                    .as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if sysfs_dirfd < 0 {
            return Err(io::Error::last_os_error());
        }
        linux_dev.sysfs_fd = sysfs_dirfd;

        // Our device handles are all set up now

        // Get all descriptors
        let all_descriptors = read_entire_sysfs_file(sysfs_dirfd, c"descriptors")?;
        if all_descriptors.is_none() {
            log::warn!(
                "Skipping device for which we could not get descriptors {}",
                dev_usb_path.to_string_lossy()
            );
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let all_descriptors = all_descriptors.unwrap();
        let (dev_desc, config_descs) = if let Some((dev_desc, config_descs)) =
            usb_ch9::ch9_core::DeviceDescriptor::from_bytes(&all_descriptors)
        {
            (dev_desc, config_descs)
        } else {
            log::warn!(
                "Skipping device for which we could not get a device descriptor {}",
                dev_usb_path.to_string_lossy()
            );
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        };

        // Get extra data
        let mut manufacturer = read_entire_sysfs_file(sysfs_dirfd, c"manufacturer")?;
        if let Some(x) = manufacturer.as_mut() {
            if x.last() == Some(&b'\n') {
                x.pop();
            }
        }
        let manufacturer = manufacturer.map(|x| String::from_utf8_lossy(&x).to_string());

        let mut product = read_entire_sysfs_file(sysfs_dirfd, c"product")?;
        if let Some(x) = product.as_mut() {
            if x.last() == Some(&b'\n') {
                x.pop();
            }
        }
        let product = product.map(|x| String::from_utf8_lossy(&x).to_string());

        let mut serial = read_entire_sysfs_file(sysfs_dirfd, c"serial")?;
        if let Some(x) = serial.as_mut() {
            if x.last() == Some(&b'\n') {
                x.pop();
            }
        }
        let serial = serial.map(|x| String::from_utf8_lossy(&x).to_string());

        // Just try to get the current config, goddamit
        let current_config =
            read_entire_sysfs_file(sysfs_dirfd, c"bConfigurationValue")?.unwrap_or_default();
        let current_config = str::from_utf8(&current_config).unwrap_or_default().trim();
        let current_config = u8::from_str_radix(current_config, 10).unwrap_or_default();

        // Only _here_ can we add the device to epoll
        // This lifetime management is all confusing *and* duplicated
        let sessionid = engine.new_sid();
        engine.add_usb_fd(dev_fd, sessionid)?;
        linux_dev.decr_event_count = &engine.actual_needed_event_sz;

        // Check capabilities and warn if we don't like them
        if !engine._printed_caps_warning.get() {
            match linux_dev.get_capabilities() {
                Ok(caps) => {
                    let wanted_caps =
                        USBDEVFS_CAP_NO_PACKET_SIZE_LIM | USBDEVFS_CAP_REAP_AFTER_DISCONNECT;
                    if caps & wanted_caps != wanted_caps {
                        log::warn!("Your kernel doesn't support important USBDEVFS features!");
                        engine._printed_caps_warning.set(true);
                    }
                }
                Err(e) => {
                    if e.raw_os_error() == Some(libc::ENOTTY) {
                        log::warn!("Your kernel is _very_ old, expect features to be unreliable!");
                    }
                    log::warn!("Getting USBDEVFS capabilities failed! {}", e);
                    engine._printed_caps_warning.set(true);
                }
            }
        }

        let mut dev = USBDevice {
            device_descriptor: *dev_desc,
            // As a hack, we just store one single entry containing all descriptors
            config_descriptors: vec![config_descs.to_vec()],
            vendor_name: manufacturer,
            product_name: product,
            serial_number: serial,

            reformatted_config_descriptors: Vec::new(),
            ep_to_idx: HashMap::new(),
            iad_map: HashMap::new(),

            opened: false,
            current_configuration_id: current_config,
            current_if_state: HashMap::new(),

            _linux_handles: linux_dev,
            _linux_epoll_fd: engine.epoll_fd,
            _linux_timeout_urb: None,
        };

        // This is only used to get the currently active alt settings
        if let Err(e) = dev.linux_probe_ifaces() {
            log::warn!("probing interfaces failed {}", e);
            // Tolerate this error
        }
        dev.reformat_config_descriptors(false);

        // At the *very* end, we can send this
        dev.send_plug_notification(sessionid, engine);

        Ok(())
    }

    // Re-determine active interface alt settings
    fn linux_probe_ifaces(&mut self) -> io::Result<()> {
        self.current_if_state = self._linux_handles.reprobe_ifaces()?;
        Ok(())
    }

    fn open_device(
        &mut self,
        sid: u64,
        txn_id: &str,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        log::debug!("device open (dummy), sid = {}, txn = {}", sid, txn_id);
        // In Linux, we effectively don't have to do anything to open/close the device
        // By the time we "detect" the device, we already have it open
        //
        // We *do* still track a virtual open state though
        self.opened = true;
        Ok(DeviceOpResult::SendCompletionNow)
    }

    fn close_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        self._check_open()?;

        log::debug!(
            "device close (by releasing interfaces), sid = {}, txn = {}",
            sid,
            txn_id
        );

        let claimed_ifs = self.current_if_state.keys().map(|x| *x).collect::<Vec<_>>();
        for iface in claimed_ifs {
            self.release_interface(sid, txn_id, iface)?;
        }
        self.opened = false;
        Ok(DeviceOpResult::SendCompletionNow)
    }

    fn reset_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        self._check_open()?;

        log::debug!("device reset, sid = {}, txn = {}", sid, txn_id);
        let ret = unsafe { libc::ioctl(self._linux_handles.dev_fd, libc::_IO(b'U' as u32, 20)) };
        if ret != 0 {
            log::warn!(
                "device reset failed, sid = {}, txn = {}, err = {} ",
                sid,
                txn_id,
                io::Error::last_os_error()
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Reset successful
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn set_configuration(
        &mut self,
        sid: u64,
        txn_id: &str,
        value: u8,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        self._check_open()?;

        log::debug!(
            "device set config 0x{:02x}, sid = {}, txn = {}",
            value,
            sid,
            txn_id
        );

        let config = value as u32;
        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<u32>(b'U' as u32, 5),
                &config,
            )
        };
        if ret != 0 {
            log::warn!(
                "SET_CONFIGURATION failed, sid = {}, txn = {}, err = {} ",
                sid,
                txn_id,
                io::Error::last_os_error(),
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Set config successful
            self.current_configuration_id = value;
            if let Err(err) = self.linux_probe_ifaces() {
                log::warn!(
                    "SET_CONFIGURATION failed reprobing interfaces, sid = {}, txn = {}, err = {} ",
                    sid,
                    txn_id,
                    err,
                );
                Err(protocol::Errors::TransferError)
            } else {
                Ok(DeviceOpResult::SendCompletionNow)
            }
        }
    }

    fn claim_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&value)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if iface_state.claimed {
            log::debug!(
                "Claiming already claimed interface 0x{:02x}, sid = {}, txn = {}",
                value,
                sid,
                txn_id
            );
            Ok(DeviceOpResult::SendCompletionNow)
        } else {
            log::debug!(
                "device claim interface 0x{:02x}, sid = {}, txn = {}",
                value,
                sid,
                txn_id
            );

            let mut claim_struct = usbdevfs_disconnect_claim {
                interface: value as u32,
                flags: USBDEVFS_DISCONNECT_CLAIM_EXCEPT_DRIVER,
                driver: [0; 256],
            };
            claim_struct.driver[..5].copy_from_slice(b"usbfs");
            let ret = unsafe {
                libc::ioctl(
                    self._linux_handles.dev_fd,
                    libc::_IOR::<usbdevfs_disconnect_claim>(b'U' as u32, 27),
                    &claim_struct,
                )
            };
            if ret != 0 {
                let err = io::Error::last_os_error();
                log::warn!(
                    "claim interface failed {}, sid = {}, txn = {}, err = {} ",
                    value,
                    sid,
                    txn_id,
                    err
                );

                if err.raw_os_error().unwrap() == libc::ENOTTY {
                    log::warn!(
                        "Your kernel is _very_ old, missing required USBDEVFS_DISCONNECT_CLAIM!"
                    );
                    Err(protocol::Errors::TransferError)
                } else if err.kind() == io::ErrorKind::ResourceBusy {
                    Err(protocol::Errors::AlreadyClaimed)
                } else {
                    Err(protocol::Errors::TransferError)
                }
            } else {
                // Claim interface successful
                iface_state.claimed = true;
                Ok(DeviceOpResult::SendCompletionNow)
            }
        }
    }

    fn release_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&value)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "device release interface 0x{:02x}, sid = {}, txn = {}",
            value,
            sid,
            txn_id
        );

        let intf = value as u32;
        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<u32>(b'U' as u32, 16),
                &intf,
            )
        };
        if ret != 0 {
            log::warn!(
                "release interface failed, sid = {}, txn = {}, err = {} ",
                sid,
                txn_id,
                io::Error::last_os_error(),
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Release interface successful
            iface_state.claimed = false;
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn set_alt_interface(&mut self, sid: u64, txn_id: &str, iface: u8, alt: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "device set alt interface 0x{:02x} 0x{:02x}, sid = {}, txn = {}",
            iface,
            alt,
            sid,
            txn_id
        );

        let set_alt_struct = usbdevfs_setinterface {
            interface: iface as u32,
            altsetting: alt as u32,
        };
        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<usbdevfs_setinterface>(b'U' as u32, 4),
                &set_alt_struct,
            )
        };
        if ret != 0 {
            // let mut mac_iface_obj = (*self._macos_ifaces[iface_state._macos_iface_idx]).borrow_mut();
            // if let Err(ret) = mac_iface_obj.SetAlternateInterface(alt) {
            log::warn!(
                "SET_INTERFACE failed {} = {:02x}, sid = {}, txn = {}, err = {} ",
                iface,
                alt,
                sid,
                txn_id,
                io::Error::last_os_error()
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Set alt interface successful
            iface_state.alt_setting = alt;
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn ctrl_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        len: u16,
        buf: Vec<u8>,
        timeout: u64,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        if request_type & 0b11111 == 1 {
            // If it's an interface transfer, check if the interface is claimed
            self._check_open()?;
            let iface = index as u8;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }
        } else if request_type & 0b11111 == 2 {
            // If it's an endpoint transfer, check if the endpoint exists and if the interface is claimed
            self._check_open()?;
            if index > 0xff {
                return Err(protocol::Errors::InvalidNumber);
            }
            let ep = index as u8;
            let iface = self
                .ep_to_idx
                .get(&(self.current_configuration_id, ep))
                .ok_or(protocol::Errors::InvalidNumber)?;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }
        }

        log::debug!(
            "control transfer, sid = {}, txn = {}, {:02x} {:02x} {:04x} {:04x} {:04x} {:02x?}",
            sid,
            txn_id,
            request_type,
            request,
            value,
            index,
            len,
            buf
        );

        // FIXME: This generates an extraneous memory copy
        // but we need to prepend the control transfer
        let mut actual_buf = Vec::with_capacity(8 + len as usize);
        actual_buf.push(request_type);
        actual_buf.push(request);
        actual_buf.extend_from_slice(&value.to_le_bytes());
        actual_buf.extend_from_slice(&index.to_le_bytes());
        actual_buf.extend_from_slice(&len.to_le_bytes());
        actual_buf.extend_from_slice(&buf);

        let urb = Box::new(usbdevfs_urb {
            type_: USBDEVFS_URB_TYPE_CONTROL,
            endpoint: 0,
            status: 0,
            flags: 0,
            buffer: actual_buf.as_mut_ptr() as *mut (),
            buffer_length: 8 + len as i32,
            actual_length: 0,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
            signr: 0,
            usercontext: ptr::null_mut(),
        });
        let urb_ptr = &*urb as *const usbdevfs_urb;

        let timeout_fd = if timeout != 0 {
            // Timeouts exist but are *only* supported for internal-use-only control transfers
            // (which means we use an ehhhhh-efficient mechanism to implement them)
            let timeoutfd =
                unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC) };
            log::debug!(
                "control transfer with timeout {} ms, fd = {}",
                timeout,
                timeoutfd
            );
            if timeoutfd >= 0 {
                let timeout_dur = Duration::from_millis(timeout);

                let timeout_struct = libc::itimerspec {
                    it_interval: libc::timespec {
                        tv_sec: 0,
                        tv_nsec: 0,
                    },
                    it_value: libc::timespec {
                        tv_sec: timeout_dur.as_secs() as i64,
                        tv_nsec: timeout_dur.subsec_nanos() as i64,
                    },
                };
                let ret = unsafe {
                    libc::timerfd_settime(timeoutfd, 0, &timeout_struct, ptr::null_mut())
                };
                if ret == 0 {
                    // Register the timeout in epoll
                    let mut epoll_timeout = libc::epoll_event {
                        events: libc::EPOLLIN as u32,
                        u64: sid | 0x80000000_00000000,
                    };
                    let ret = unsafe {
                        libc::epoll_ctl(
                            self._linux_epoll_fd,
                            libc::EPOLL_CTL_ADD,
                            timeoutfd,
                            &mut epoll_timeout,
                        )
                    };
                    if ret < 0 {
                        log::warn!(
                            "failed to set up timeout (epoll), sid = {}, txn = {}, err = {}",
                            sid,
                            txn_id,
                            io::Error::last_os_error()
                        );
                    } else {
                        // Timeout is *finally* all set up
                        if self._linux_timeout_urb.is_some() {
                            log::warn!(
                                "BAD BAD BAD! Multiple URBs with timeout in flight, sid = {}, txn = {}",
                                sid,
                                txn_id,
                            );
                        }

                        self._linux_timeout_urb = Some(urb_ptr);
                    }

                    timeoutfd
                } else {
                    log::warn!(
                        "failed to set up timeout (settime), sid = {}, txn = {}, err = {}",
                        sid,
                        txn_id,
                        io::Error::last_os_error()
                    );

                    timeoutfd
                }
            } else {
                log::warn!(
                    "failed to set up timeout (create), sid = {}, txn = {}, err = {}",
                    sid,
                    txn_id,
                    io::Error::last_os_error()
                );

                -1
            }
        } else {
            -1
        };

        let mut urbwrapper = Box::new(LinuxURBWrapper {
            txn_id: txn_id.to_owned(),
            dir,
            buf: actual_buf,
            urb,
            _timeout_fd: timeout_fd,
        });
        urbwrapper.urb.usercontext = &*urbwrapper as *const LinuxURBWrapper as *mut ();
        let _urbwrapper = Box::into_raw(urbwrapper);

        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<usbdevfs_urb>(b'U' as u32, 10),
                urb_ptr,
            )
        };
        if ret == 0 {
            Ok(DeviceOpResult::ManualCompletion)
        } else {
            log::warn!(
                "SUBMITURB failed, sid = {}, txn = {}, err = {}",
                sid,
                txn_id,
                io::Error::last_os_error()
            );
            Err(protocol::Errors::TransferError)
        }
    }

    fn data_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        len: u32,
        mut buf: Vec<u8>,
    ) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "bulk/interrupt transfer, sid = {}, txn = {}, {:02x} {:08x} {:02x?}",
            sid,
            txn_id,
            ep,
            len,
            buf
        );

        assert!(buf.capacity() >= len as usize);
        let urb = Box::new(usbdevfs_urb {
            // The Linux kernel permits sending bulk URBs at interrupt endpoints
            // We rely on this hack so that we don't have to bother looking up what the actual type is
            type_: USBDEVFS_URB_TYPE_BULK,
            endpoint: ep,
            status: 0,
            flags: 0,
            buffer: buf.as_mut_ptr() as *mut (),
            buffer_length: len as i32,
            actual_length: 0,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
            signr: 0,
            usercontext: ptr::null_mut(),
        });
        let urb_ptr = &*urb as *const usbdevfs_urb;

        let mut urbwrapper = Box::new(LinuxURBWrapper {
            txn_id: txn_id.to_owned(),
            dir,
            buf,
            urb,
            _timeout_fd: -1,
        });
        urbwrapper.urb.usercontext = &*urbwrapper as *const LinuxURBWrapper as *mut ();
        let _urbwrapper = Box::into_raw(urbwrapper);

        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<usbdevfs_urb>(b'U' as u32, 10),
                urb_ptr,
            )
        };
        if ret == 0 {
            Ok(DeviceOpResult::ManualCompletion)
        } else {
            log::warn!(
                "SUBMITURB failed, sid = {}, txn = {}, err = {}",
                sid,
                txn_id,
                io::Error::last_os_error()
            );
            Err(protocol::Errors::TransferError)
        }
    }

    fn clear_halt(&mut self, sid: u64, txn_id: &str, ep: u8) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!("clear halt, sid = {}, txn = {}, {:02x}", sid, txn_id, ep,);

        let ep = ep as u32;
        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<u32>(b'U' as u32, 21),
                &ep,
            )
        };
        if ret != 0 {
            log::warn!(
                "CLEAR_HALT failed, sid = {}, txn = {}, err = {} ",
                sid,
                txn_id,
                io::Error::last_os_error(),
            );
            Err(protocol::Errors::TransferError)
        } else {
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn isoc_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        total_len: usize,
        pkt_len: Vec<u32>,
        mut buf: Vec<u8>,
    ) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "isoc transfer, sid = {}, txn = {}, {:02x} {:08x} {:?} {:02x?}",
            sid,
            txn_id,
            ep,
            total_len,
            pkt_len,
            buf
        );

        assert!(total_len <= buf.capacity());
        let mut urb = usbdevfs_urb_with_iso::new(pkt_len.len());
        urb.urb.type_ = USBDEVFS_URB_TYPE_ISO;
        urb.urb.endpoint = ep;
        urb.urb.flags = USBDEVFS_URB_ISO_ASAP;
        urb.urb.buffer = buf.as_mut_ptr() as *mut ();
        urb.urb.number_of_packets = pkt_len.len() as i32;
        for (i, &pkt_len_i) in pkt_len.iter().enumerate() {
            urb.iso_frame_desc[i].length = pkt_len_i;
        }
        let urb_ptr = &*urb as *const usbdevfs_urb_with_iso;

        let mut urbwrapper = Box::new(LinuxIsoURBWrapper {
            txn_id: txn_id.to_owned(),
            dir,
            buf,
            urb,
        });
        urbwrapper.urb.urb.usercontext = &*urbwrapper as *const LinuxIsoURBWrapper as *mut ();
        let _urbwrapper = Box::into_raw(urbwrapper);

        let ret = unsafe {
            libc::ioctl(
                self._linux_handles.dev_fd,
                libc::_IOR::<usbdevfs_urb>(b'U' as u32, 10),
                urb_ptr,
            )
        };
        if ret == 0 {
            Ok(DeviceOpResult::ManualCompletion)
        } else {
            log::warn!(
                "SUBMITURB failed, sid = {}, txn = {}, err = {}",
                sid,
                txn_id,
                io::Error::last_os_error()
            );
            Err(protocol::Errors::TransferError)
        }
    }
}

#[cfg(target_os = "macos")]
impl USBDevice {
    #[allow(non_snake_case)]
    pub fn setup(
        obj: io_object_t,
        sessionid: u64,
        engine: Pin<&USBStubEngine>,
    ) -> Result<(), libc::kern_return_t> {
        // These are available in IOKit, but not on the interface API.
        // None of these gets (here or below) should ever fail on versions of macOS we support
        // (but, as a hack, we ignore failures here to avoid leaking the object).
        let bcdUSB = get_bcd_usb(obj).unwrap_or_default();
        let bMaxPacketSize0 = get_max_pkt_0(obj).unwrap_or_default();

        let str_manuf = get_usb_cached_string(obj, "USB Vendor Name");
        let str_product = get_usb_cached_string(obj, "USB Product Name");
        let str_sn = get_usb_cached_string(obj, "USB Serial Number");

        // Ownership is actually converted here
        let mut usb_dev = unsafe { IOUSBDeviceStruct::new(obj) };

        // Create async event notification
        let mach_port = usb_dev.CreateDeviceAsyncPort()?;
        engine.add_mach_port(mach_port).map_err(|_| 0)?; // FIXME
        usb_dev.1 = &engine.actual_needed_event_sz;

        // Get device descriptor fields
        let bDeviceClass = usb_dev.GetDeviceClass()?;
        let bDeviceSubClass = usb_dev.GetDeviceSubClass()?;
        let bDeviceProtocol = usb_dev.GetDeviceProtocol()?;
        let idVendor = usb_dev.GetDeviceVendor()?;
        let idProduct = usb_dev.GetDeviceProduct()?;
        let bcdDevice = usb_dev.GetDeviceReleaseNumber()?;
        let iManufacturer = usb_dev.USBGetManufacturerStringIndex()?;
        let iProduct = usb_dev.USBGetProductStringIndex()?;
        let iSerialNumber = usb_dev.USBGetSerialNumberStringIndex()?;
        let bNumConfigurations = usb_dev.GetNumberOfConfigurations()?;

        let dev_desc = usb_ch9::ch9_core::DeviceDescriptor {
            bLength: std::mem::size_of::<usb_ch9::ch9_core::DeviceDescriptor>() as u8,
            bDescriptorType: usb_ch9::ch9_core::descriptor_types::DEVICE,
            bcdUSB,
            bDeviceClass,
            bDeviceSubClass,
            bDeviceProtocol,
            bMaxPacketSize0,
            idVendor,
            idProduct,
            bcdDevice,
            iManufacturer,
            iProduct,
            iSerialNumber,
            bNumConfigurations,
        };

        // Get configuration descriptors
        let mut config_descs = Vec::new();
        for i in 0..bNumConfigurations {
            let conf_desc = usb_dev.GetConfigurationDescriptorPtr(i)?;

            // SAFETY: We read the initial configuration descriptor and then use its length
            // (which is what everybody just has to do here)
            let cfg_desc_initial = conf_desc as *const usb_ch9::ch9_core::ConfigDescriptor;
            let total_desc_len = unsafe { (*cfg_desc_initial).wTotalLength as usize };
            let config_desc =
                unsafe { std::slice::from_raw_parts(conf_desc as *const u8, total_desc_len) };

            config_descs.push(config_desc.to_owned());
        }

        // Get current state
        // TODO: Does this generate unnecessary wakes and/or bus traffic?
        let current_config = usb_dev.GetConfiguration()?;

        let mut dev = USBDevice {
            device_descriptor: dev_desc,
            config_descriptors: config_descs,
            vendor_name: str_manuf,
            product_name: str_product,
            serial_number: str_sn,

            reformatted_config_descriptors: Vec::new(),
            ep_to_idx: HashMap::new(),
            iad_map: HashMap::new(),

            opened: false,
            current_configuration_id: current_config,
            current_if_state: HashMap::new(),

            _macos_ep_to_idx: HashMap::new(),
            _macos_dev: usb_dev,
            _macos_ifaces: Vec::new(),
        };

        // Open a handle to all the interfaces
        dev.macos_probe_ifaces(engine)?;
        dev.reformat_config_descriptors(false);

        // At the *very* end, we can send this
        dev.send_plug_notification(sessionid, engine);

        Ok(())
    }

    /// Open or re-open all interface handles, when switching configuration
    pub(crate) fn macos_probe_ifaces(
        &mut self,
        engine: Pin<&USBStubEngine>,
    ) -> Result<(), libc::kern_return_t> {
        // Make sure we close all the existing handles first
        self._macos_ifaces.clear();
        self._macos_ep_to_idx.clear();

        let mut if_states = HashMap::new();

        // Open interfaces
        let mut ifaces = Vec::new();
        let mut iface_idx = 0;
        let iter_ifaces = self
            ._macos_dev
            .CreateInterfaceIterator(&IOUSBFindInterfaceRequest {
                bInterfaceClass: 0xffff,
                bInterfaceSubClass: 0xffff,
                bInterfaceProtocol: 0xffff,
                bAlternateSetting: 0xffff,
            })?;

        let mut iface_iokit;
        loop {
            iface_iokit = unsafe { IOIteratorNext(iter_ifaces) };
            if iface_iokit.0 == 0 {
                break;
            }

            // Takes ownership
            let mut usb_iface = unsafe { IOUSBInterfaceStruct::new(iface_iokit) };

            // Create async event notification
            let mach_port = usb_iface.CreateInterfaceAsyncPort()?;
            engine.add_mach_port(mach_port).map_err(|_| 0)?; // FIXME
            usb_iface.1 = &engine.actual_needed_event_sz;

            // NOTE: The macOS SDK documentation is either wrong or confusing.
            // The GetInterfaceNumber function indeed returns the bInterfaceNumber
            // which you might expect. It does *not* return a plain 0-based index
            // according to the ordering of the config descriptor.
            let iface_num = usb_iface.GetInterfaceNumber()?;
            let alt_setting = usb_iface.GetAlternateSetting()?;
            let old = if_states.insert(
                iface_num,
                USBInterfaceState {
                    alt_setting,
                    claimed: false,
                    _macos_iface_idx: iface_idx,
                    _macos_ep_addrs: Vec::new(),
                },
            );
            if old.is_some() {
                log::warn!("Duplicate interface?? {}", iface_num);
            }

            ifaces.push(Rc::new(RefCell::new(usb_iface)));
            iface_idx += 1;
        }
        unsafe { IOObjectRelease(iter_ifaces) };

        self.current_if_state = if_states;
        self._macos_ifaces = ifaces;

        Ok(())
    }

    fn open_device(
        &mut self,
        sid: u64,
        txn_id: &str,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        if self.opened {
            log::debug!(
                "Opening already opened device, sid = {}, txn = {}",
                sid,
                txn_id
            );
            Ok(DeviceOpResult::SendCompletionNow)
        } else {
            log::debug!("device open, sid = {}, txn = {}", sid, txn_id);
            if let Err(ret) = self._macos_dev.USBDeviceOpen() {
                log::warn!(
                    "USBDeviceOpen failed, sid = {}, txn = {}, ret = {:08x} ",
                    sid,
                    txn_id,
                    ret
                );
                if ret == kIOReturnExclusiveAccess {
                    Err(protocol::Errors::AlreadyClaimed)
                } else {
                    Err(protocol::Errors::TransferError)
                }
            } else {
                // Open successful
                self.opened = true;
                Ok(DeviceOpResult::SendCompletionNow)
            }
        }
    }

    fn close_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        self._check_open()?;

        log::debug!("device close, sid = {}, txn = {}", sid, txn_id);

        // Release all interfaces before closing
        for (_, iface_state) in self.current_if_state.iter_mut() {
            if iface_state.claimed {
                log::debug!(" -> releasing interface {}", iface_state._macos_iface_idx);

                let mut mac_iface_obj =
                    (*self._macos_ifaces[iface_state._macos_iface_idx]).borrow_mut();
                if let Err(ret) = mac_iface_obj.USBInterfaceClose() {
                    log::warn!(
                        "USBInterfaceClose failed {}, sid = {}, txn = {}, ret = {:08x} ",
                        iface_state._macos_iface_idx,
                        sid,
                        txn_id,
                        ret
                    );
                    // If closing an interface fails, don't mark it as closed
                    // but also don't report the failure to the host
                    // FIXME: How is this supposed to work?
                } else {
                    iface_state.claimed = false;
                }
            }
        }

        if let Err(ret) = self._macos_dev.USBDeviceClose() {
            log::warn!(
                "USBDeviceClose failed, sid = {}, txn = {}, ret = {:08x} ",
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Close successful
            self.opened = false;
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn reset_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        self._check_open()?;

        log::debug!("device reset, sid = {}, txn = {}", sid, txn_id);
        if let Err(ret) = self._macos_dev.USBDeviceReEnumerate(0) {
            log::warn!(
                "USBDeviceReEnumerate failed, sid = {}, txn = {}, ret = {:08x} ",
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Reset successful
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn set_configuration(
        &mut self,
        sid: u64,
        txn_id: &str,
        value: u8,
        engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        self._check_open()?;

        log::debug!(
            "device set config 0x{:02x}, sid = {}, txn = {}",
            value,
            sid,
            txn_id
        );
        if let Err(ret) = self._macos_dev.SetConfiguration(value) {
            log::warn!(
                "SetConfiguration failed, sid = {}, txn = {}, ret = {:08x} ",
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Set config successful
            self.current_configuration_id = value;
            if let Err(ret) = self.macos_probe_ifaces(engine) {
                log::warn!(
                    "SetConfiguration failed reopening interfaces, sid = {}, txn = {}, ret = {:08x} ",
                    sid,
                    txn_id,
                    ret
                );
                Err(protocol::Errors::TransferError)
            } else {
                Ok(DeviceOpResult::SendCompletionNow)
            }
        }
    }

    fn claim_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&value)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if iface_state.claimed {
            log::debug!(
                "Claiming already claimed interface 0x{:02x}, sid = {}, txn = {}",
                value,
                sid,
                txn_id
            );
            Ok(DeviceOpResult::SendCompletionNow)
        } else {
            log::debug!(
                "device claim interface 0x{:02x}, sid = {}, txn = {}",
                value,
                sid,
                txn_id
            );

            let mut mac_iface_obj =
                (*self._macos_ifaces[iface_state._macos_iface_idx]).borrow_mut();
            if let Err(ret) = mac_iface_obj.USBInterfaceOpen() {
                log::warn!(
                    "USBInterfaceOpen failed {}, sid = {}, txn = {}, ret = {:08x} ",
                    iface_state._macos_iface_idx,
                    sid,
                    txn_id,
                    ret
                );
                if ret == kIOReturnExclusiveAccess {
                    Err(protocol::Errors::AlreadyClaimed)
                } else {
                    Err(protocol::Errors::TransferError)
                }
            } else {
                // Claim interface successful
                iface_state.claimed = true;

                // Update this !@#$ list
                assert_eq!(iface_state._macos_ep_addrs.len(), 0);
                let ep_nums = mac_iface_obj.get_ep_addrs();
                for (piperef_m1, ep) in ep_nums.iter().enumerate() {
                    let old = self
                        ._macos_ep_to_idx
                        .insert(*ep, (iface_state._macos_iface_idx, (piperef_m1 + 1) as u8));
                    if old.is_some() {
                        log::warn!("Duplicate endpoint?! {:02x}", ep);
                    }
                }
                iface_state._macos_ep_addrs = ep_nums;

                Ok(DeviceOpResult::SendCompletionNow)
            }
        }
    }

    fn release_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&value)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "device release interface 0x{:02x}, sid = {}, txn = {}",
            value,
            sid,
            txn_id
        );

        let mut mac_iface_obj = (*self._macos_ifaces[iface_state._macos_iface_idx]).borrow_mut();
        if let Err(ret) = mac_iface_obj.USBInterfaceClose() {
            log::warn!(
                "USBInterfaceClose failed {}, sid = {}, txn = {}, ret = {:08x} ",
                iface_state._macos_iface_idx,
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Release interface successful
            iface_state.claimed = false;

            // Update this !@#$ list
            for ep in iface_state._macos_ep_addrs.drain(..) {
                self._macos_ep_to_idx.remove(&ep);
            }

            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn set_alt_interface(&mut self, sid: u64, txn_id: &str, iface: u8, alt: u8) -> DeviceResult {
        self._check_open()?;

        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }

        log::debug!(
            "device set alt interface 0x{:02x} 0x{:02x}, sid = {}, txn = {}",
            iface,
            alt,
            sid,
            txn_id
        );

        let mut mac_iface_obj = (*self._macos_ifaces[iface_state._macos_iface_idx]).borrow_mut();
        if let Err(ret) = mac_iface_obj.SetAlternateInterface(alt) {
            log::warn!(
                "SetAlternateInterface failed {} = {:02x}, sid = {}, txn = {}, ret = {:08x} ",
                iface_state._macos_iface_idx,
                alt,
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            // Set alt interface successful
            iface_state.alt_setting = alt;

            // Update this !@#$ list
            for ep in iface_state._macos_ep_addrs.drain(..) {
                self._macos_ep_to_idx.remove(&ep);
            }
            let ep_nums = mac_iface_obj.get_ep_addrs();
            for (piperef_m1, ep) in ep_nums.iter().enumerate() {
                let old = self
                    ._macos_ep_to_idx
                    .insert(*ep, (iface_state._macos_iface_idx, (piperef_m1 + 1) as u8));
                if old.is_some() {
                    log::warn!("Duplicate endpoint?! {:02x}", ep);
                }
            }
            iface_state._macos_ep_addrs = ep_nums;

            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn ctrl_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        len: u16,
        buf: Vec<u8>,
        timeout: u64,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        // As a special exception, "device" control transfers do *not* require the device to be open
        // This is a hack for the "interal" page access. The JS API does it's *own* check for this,
        // so user pages do still need to have the device open.

        if request_type & 0b11111 == 1 {
            // If it's an interface transfer, check if the interface is claimed
            self._check_open()?;
            let iface = index as u8;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }
        } else if request_type & 0b11111 == 2 {
            // If it's an endpoint transfer, check if the endpoint exists and if the interface is claimed
            self._check_open()?;
            if index > 0xff {
                return Err(protocol::Errors::InvalidNumber);
            }
            let ep = index as u8;
            let iface = self
                .ep_to_idx
                .get(&(self.current_configuration_id, ep))
                .ok_or(protocol::Errors::InvalidNumber)?;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }
            let _ = self
                ._macos_ep_to_idx
                .get(&ep)
                .ok_or(protocol::Errors::InvalidNumber)?;
        }

        log::debug!(
            "control transfer, sid = {}, txn = {}, {:02x} {:02x} {:04x} {:04x} {:04x} {:02x?}",
            sid,
            txn_id,
            request_type,
            request,
            value,
            index,
            len,
            buf
        );

        if let Err(ret) = self._macos_dev.ctrl_xfer(
            txn_id,
            buf,
            request_type,
            request,
            value,
            index,
            len,
            timeout as u32,
            dir,
        ) {
            // NOTE: A removed device doesn't seem to generate errors here
            log::warn!(
                "DeviceRequestAsyncTO failed, sid = {}, txn = {}, ret = {:08x} ",
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            Ok(DeviceOpResult::ManualCompletion)
        }
    }

    fn data_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        len: u32,
        buf: Vec<u8>,
    ) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }
        let (macos_idx, macos_piperef) = self
            ._macos_ep_to_idx
            .get(&ep)
            .ok_or(protocol::Errors::InvalidNumber)?;

        log::debug!(
            "bulk/interrupt transfer, sid = {}, txn = {}, {:02x} {:08x} {:02x?}",
            sid,
            txn_id,
            ep,
            len,
            buf
        );

        let iface2 = self._macos_ifaces[*macos_idx].clone();
        let mut mac_iface_obj = (*self._macos_ifaces[*macos_idx]).borrow_mut();
        if let Err(ret) =
            mac_iface_obj.data_xfer(iface2, txn_id, buf, *macos_piperef, len as u32, dir)
        {
            log::warn!(
                "Read/WritePipeAsync failed ep 0x{:02x}, sid = {}, txn = {}, ret = {:08x} ",
                ep,
                sid,
                txn_id,
                ret
            );

            if ret == kIOUSBPipeStalled {
                Err(protocol::Errors::Stall)
            } else {
                Err(protocol::Errors::TransferError)
            }
        } else {
            Ok(DeviceOpResult::ManualCompletion)
        }
    }

    fn clear_halt(&mut self, sid: u64, txn_id: &str, ep: u8) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }
        let (macos_idx, macos_piperef) = self
            ._macos_ep_to_idx
            .get(&ep)
            .ok_or(protocol::Errors::InvalidNumber)?;

        log::debug!("clear halt, sid = {}, txn = {}, {:02x}", sid, txn_id, ep,);

        let mut mac_iface_obj = (*self._macos_ifaces[*macos_idx]).borrow_mut();
        if let Err(ret) = mac_iface_obj.ClearPipeStallBothEnds(*macos_piperef) {
            log::warn!(
                "ClearPipeStallBothEnds failed ep 0x{:02x}, sid = {}, txn = {}, ret = {:08x} ",
                ep,
                sid,
                txn_id,
                ret
            );
            Err(protocol::Errors::TransferError)
        } else {
            Ok(DeviceOpResult::SendCompletionNow)
        }
    }

    fn isoc_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        total_len: usize,
        pkt_len: Vec<u32>,
        buf: Vec<u8>,
    ) -> DeviceResult {
        self._check_open()?;

        let iface = self
            .ep_to_idx
            .get(&(self.current_configuration_id, ep))
            .ok_or(protocol::Errors::InvalidNumber)?;
        let iface_state = self
            .current_if_state
            .get_mut(&iface)
            .ok_or(protocol::Errors::InvalidNumber)?;
        if !iface_state.claimed {
            return Err(protocol::Errors::InvalidState);
        }
        let (macos_idx, macos_piperef) = self
            ._macos_ep_to_idx
            .get(&ep)
            .ok_or(protocol::Errors::InvalidNumber)?;

        log::debug!(
            "isoc transfer, sid = {}, txn = {}, {:02x} {:08x} {:?} {:02x?}",
            sid,
            txn_id,
            ep,
            total_len,
            pkt_len,
            buf
        );

        let iface2 = self._macos_ifaces[*macos_idx].clone();
        let mut mac_iface_obj = (*self._macos_ifaces[*macos_idx]).borrow_mut();

        if let Err(ret) =
            mac_iface_obj.isoc_xfer(iface2, txn_id, buf, *macos_piperef, pkt_len, total_len, dir)
        {
            log::warn!(
                "Read/WriteIsocPipeAsync failed ep 0x{:02x}, sid = {}, txn = {}, ret = {:08x} ",
                ep,
                sid,
                txn_id,
                ret
            );

            Err(protocol::Errors::TransferError)
        } else {
            Ok(DeviceOpResult::ManualCompletion)
        }
    }
}

#[cfg(windows)]
impl USBDevice {
    pub fn setup(notif: WinHotplugNotification, engine: Pin<&USBStubEngine>) {
        if let WinHotplugNotification::NewInterface {
            session_id,
            interface_no,
            interface_path,
            whole_device,
            device_info,
        } = notif
        {
            if device_info.is_none() {
                if let Some(existing_device) = engine.usb_devices.borrow_mut().get_mut(&session_id)
                {
                    let orig = existing_device
                        ._win_iface_paths
                        .insert(interface_no, interface_path);
                    if orig.is_some() {
                        log::warn!(
                            "Adding an interface {:02x} to a device {} we *already* have!",
                            interface_no,
                            session_id
                        );
                    }
                } else {
                    log::warn!(
                        "Ignoring adding an interface {:02x} {:?} to a device {} we don't have!",
                        interface_no,
                        interface_path,
                        session_id
                    );
                }
                return;
            }
            let dev_info = device_info.unwrap();

            let vendor_name = if dev_info.dev_desc.iManufacturer != 0 {
                if let Some(s) = dev_info.string_descs.get(&dev_info.dev_desc.iManufacturer) {
                    if let Some((s, _)) = usb_ch9::ch9_core::StringDescriptor::from_bytes(&s) {
                        Some(
                            s.payload()
                                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                                .collect::<String>(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let product_name = if dev_info.dev_desc.iProduct != 0 {
                if let Some(s) = dev_info.string_descs.get(&dev_info.dev_desc.iProduct) {
                    if let Some((s, _)) = usb_ch9::ch9_core::StringDescriptor::from_bytes(&s) {
                        Some(
                            s.payload()
                                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                                .collect::<String>(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let serial_number = if dev_info.dev_desc.iSerialNumber != 0 {
                if let Some(s) = dev_info.string_descs.get(&dev_info.dev_desc.iSerialNumber) {
                    if let Some((s, _)) = usb_ch9::ch9_core::StringDescriptor::from_bytes(&s) {
                        Some(
                            s.payload()
                                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                                .collect::<String>(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let mut dev = USBDevice {
                device_descriptor: dev_info.dev_desc,
                config_descriptors: dev_info.config_descs,
                vendor_name,
                product_name,
                serial_number,

                reformatted_config_descriptors: Vec::new(),
                ep_to_idx: HashMap::new(),
                iad_map: HashMap::new(),

                opened: false,
                current_configuration_id: dev_info.current_config,
                current_if_state: HashMap::new(),

                _win_iface_paths: HashMap::new(),
                _win_iface_handles: HashMap::new(),
                _win_string_cache: dev_info.string_descs,
                _win_bos_desc: dev_info.bos_desc,
            };

            // This is the only interface we know about so far
            dev._win_iface_paths.insert(interface_no, interface_path);

            dev.reformat_config_descriptors(true);

            // If we have the whole device, rewrite the IAD map so that *everything* maps to 0
            if whole_device {
                let mut iad_map = HashMap::new();
                for cfg_desc in &dev.reformatted_config_descriptors {
                    if cfg_desc.bConfigurationValue == dev.current_configuration_id {
                        for if_desc in &cfg_desc.interfaces {
                            iad_map.insert(if_desc.bInterfaceNumber, 0);
                        }
                    }
                }
                dev.iad_map = iad_map;
            }

            // At the *very* end, we can send this
            dev.send_plug_notification(session_id, engine);
        } else {
            panic!("passed wrong notification to setup!")
        }
    }

    // Try and open _any_ path we have, until it succeeds
    fn _open_some_interface(&mut self, engine: Pin<&USBStubEngine>) -> Option<(u8, WinUSBHandle)> {
        let mut opened = None;
        let mut ifaces_to_try = self._win_iface_paths.keys().collect::<Vec<_>>();
        ifaces_to_try.sort();
        for if_no in ifaces_to_try {
            let if_path = self._win_iface_paths.get(if_no).unwrap();
            log::debug!("Trying {:?}", if_path);
            match WinUSBHandle::open(if_path) {
                Ok(handle) => {
                    opened = Some((*if_no, handle));
                    break;
                }
                Err(err) => {
                    log::debug!("Failed to open {:?}: {}", if_path, err);
                }
            }
        }

        if let Some((_, handles)) = &opened {
            let ret = unsafe {
                windows_sys::Win32::System::IO::CreateIoCompletionPort(
                    handles.raw_handle,
                    engine.iocp,
                    0,
                    0,
                )
            };
            if ret.is_null() {
                let err = io::Error::last_os_error();
                log::warn!("Failed to associate IOCP! {}", err);
                return None;
            }
        }

        opened
    }

    fn open_device(&mut self, sid: u64, txn_id: &str, engine: Pin<&USBStubEngine>) -> DeviceResult {
        if self.opened {
            log::debug!(
                "Opening already opened device, sid = {}, txn = {}",
                sid,
                txn_id
            );
            Ok(DeviceOpResult::SendCompletionNow)
        } else {
            log::debug!("device open, sid = {}, txn = {}", sid, txn_id);

            if self._win_iface_handles.len() > 0 {
                // This should never happen, but we ignore it if it idoes
                log::warn!("For some reason, we appear to already have a handle open?!");
            }

            if let Some((if_no, handle)) = self._open_some_interface(engine) {
                // Open successful
                log::debug!("Opened interface {:02x}", if_no);
                self._win_iface_handles.insert(if_no, handle);
                self.opened = true;
                Ok(DeviceOpResult::SendCompletionNow)
            } else {
                log::warn!(
                    "Couldn't open _any_ handles, sid = {}, txn = {}",
                    sid,
                    txn_id,
                );
                Err(protocol::Errors::TransferError)
            }
        }
    }

    fn close_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        todo!()
    }

    fn reset_device(&mut self, sid: u64, txn_id: &str) -> DeviceResult {
        self._check_open()?;
        log::debug!("device reset (dummy), sid = {}, txn = {}", sid, txn_id);
        // Windows is unable to reset the device
        Err(protocol::Errors::TransferError)
    }

    fn set_configuration(
        &mut self,
        sid: u64,
        txn_id: &str,
        value: u8,
        _engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        self._check_open()?;
        log::debug!(
            "device set config (dummy) 0x{:02x}, sid = {}, txn = {}",
            value,
            sid,
            txn_id
        );
        // Windows is unable to change the device configuration
        Err(protocol::Errors::TransferError)
    }

    fn claim_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        todo!()
    }

    fn release_interface(&mut self, sid: u64, txn_id: &str, value: u8) -> DeviceResult {
        todo!()
    }

    fn set_alt_interface(&mut self, sid: u64, txn_id: &str, iface: u8, alt: u8) -> DeviceResult {
        todo!()
    }

    fn ctrl_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        len: u16,
        buf: Vec<u8>,
        timeout: u64,
        engine: Pin<&USBStubEngine>,
    ) -> DeviceResult {
        // As a _horrible hack_, before *anything else*, answer get_descriptor requests from cache
        if request_type == 0x80 && request == usb_ch9::ch9_core::requests::GET_DESCRIPTOR {
            let mut answered = None;
            match (value >> 8) as u8 {
                usb_ch9::ch9_core::descriptor_types::DEVICE => {
                    answered = Some(self.device_descriptor.to_bytes());
                }
                usb_ch9::ch9_core::descriptor_types::CONFIGURATION => {
                    let which = (value & 0xff) as usize;
                    if which < self.config_descriptors.len() {
                        answered = Some(&self.config_descriptors[which]);
                    }
                }
                usb_ch9::ch9_core::descriptor_types::STRING => {
                    let which = (value & 0xff) as u8;
                    if let Some(cached) = self._win_string_cache.get(&which) {
                        answered = Some(&cached);
                    }
                }
                15 => {
                    // BOS
                    if let Some(bos) = &self._win_bos_desc {
                        answered = Some(&bos);
                    }
                }
                _ => {}
            }

            if let Some(answered) = answered {
                let actual_len = usize::min(answered.len(), len as usize);
                let answered = &answered[..actual_len];

                log::debug!("answering request from cache: {:?}", answered);

                let data = Some(URL_SAFE_NO_PAD.encode(answered));
                let notif = crate::protocol::ResponseMessage::RequestComplete {
                    txn_id: txn_id.to_string(),
                    babble: false,
                    data,
                    bytes_written: actual_len as u64,
                };
                let notif = serde_json::to_string(&notif).unwrap();
                write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
                return Ok(DeviceOpResult::ManualCompletion);
            }
        }

        // Otherwise it's _not_ something we have cached
        let interface_handle;
        let mut _internal_use_interface_handle;
        if request_type & 0b11111 == 1 {
            // If it's an interface transfer, check if the interface is claimed
            self._check_open()?;
            let iface = index as u8;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }

            todo!()
        } else if request_type & 0b11111 == 2 {
            // If it's an endpoint transfer, check if the endpoint exists and if the interface is claimed
            self._check_open()?;
            if index > 0xff {
                return Err(protocol::Errors::InvalidNumber);
            }
            let ep = index as u8;
            let iface = self
                .ep_to_idx
                .get(&(self.current_configuration_id, ep))
                .ok_or(protocol::Errors::InvalidNumber)?;
            let iface_state = self
                .current_if_state
                .get_mut(&iface)
                .ok_or(protocol::Errors::InvalidNumber)?;
            if !iface_state.claimed {
                return Err(protocol::Errors::InvalidState);
            }

            todo!()
        } else {
            // A device or a other/reserved request, so we need _something_ open
            // (unless it's an internal-use-only request, as another huge hack)

            // Checking the timeout is the hack we use to detect internal-use-only transfers
            if timeout != 0 {
                if self._win_iface_handles.len() > 0 {
                    // This should never happen, but we ignore it if it idoes
                    log::warn!("For some reason, we appear to already have a handle open?!");
                }

                if let Some((if_no, handle)) = self._open_some_interface(engine) {
                    // Open successful
                    log::debug!(
                        "Opened interface {:02x} (for internal control transfers)",
                        if_no
                    );
                    _internal_use_interface_handle = handle;
                    interface_handle = &mut _internal_use_interface_handle;
                } else {
                    log::warn!(
                        "Couldn't open _any_ handles, sid = {}, txn = {}",
                        sid,
                        txn_id,
                    );
                    return Err(protocol::Errors::TransferError);
                }
            } else {
                self._check_open()?;
                // Just pick the first handle that pops out
                interface_handle = self._win_iface_handles.values_mut().next().unwrap();
            }
        }

        log::debug!(
            "control transfer, sid = {}, txn = {}, {:02x} {:02x} {:04x} {:04x} {:04x} {:02x?}",
            sid,
            txn_id,
            request_type,
            request,
            value,
            index,
            len,
            buf
        );

        if let Err(err) = interface_handle.ctrl_xfer(
            txn_id,
            buf,
            request_type,
            request,
            value,
            index,
            len,
            timeout as u32,
            dir,
        ) {
            log::warn!(
                "WinUsb_ControlTransfer failed, sid = {}, txn = {}, err = {} ",
                sid,
                txn_id,
                err
            );
            Err(protocol::Errors::TransferError)
        } else {
            Ok(DeviceOpResult::ManualCompletion)
        }
    }

    fn data_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        len: u32,
        buf: Vec<u8>,
    ) -> DeviceResult {
        todo!()
    }

    fn clear_halt(&mut self, sid: u64, txn_id: &str, ep: u8) -> DeviceResult {
        todo!()
    }

    fn isoc_xfer(
        &mut self,
        sid: u64,
        txn_id: &str,
        dir: USBTransferDirection,
        ep: u8,
        total_len: usize,
        pkt_len: Vec<u32>,
        buf: Vec<u8>,
    ) -> DeviceResult {
        todo!()
    }
}

#[cfg(target_os = "linux")]
const RUNLOOP_EPOLL_STDIN: u64 = -1i64 as u64;
#[cfg(target_os = "linux")]
const RUNLOOP_EPOLL_UDEV: u64 = -2i64 as u64;

/// Main struct holding all of the state for our operations
#[derive(Debug)]
pub struct USBStubEngine {
    /// Map from session IDs to devices
    usb_devices: RefCell<HashMap<u64, USBDevice>>,

    // Windows is _shockingly_ insistent on using threads everywhere,
    // so we use channels to funnel a bunch of stuff back onto the main thread.
    #[cfg(windows)]
    stdin_reader: WinStdinReader,
    #[cfg(windows)]
    notifcation_handler: WinNotificationHandler,
    // We also process IOCPs on a background thread, because Windows is insistent.
    // Our USB transfer structs are self-contained, so this Should Be Fine (TM)
    #[cfg(windows)]
    iocp: windows_sys::Win32::Foundation::HANDLE,

    #[cfg(not(windows))]
    // As we watch more things, we make the event buffer bigger.
    // But we never make it smaller, so this field keeps track of
    // how many events we _actually_ need.
    //
    // This is used by both epoll (Linux) and kqueue (macOS)
    actual_needed_event_sz: Cell<usize>,

    #[cfg(target_os = "linux")]
    /// Number which will be incremented to uniquely identify devices,
    /// since Linux has no better way to do that
    next_session_id: Cell<u64>,
    #[cfg(target_os = "linux")]
    epoll_fd: i32,
    #[cfg(target_os = "linux")]
    epoll_buf: RefCell<Vec<libc::epoll_event>>,
    #[cfg(target_os = "linux")]
    udev_connection: UdevNetlinkSocket,
    #[cfg(target_os = "linux")]
    _printed_caps_warning: Cell<bool>,

    #[cfg(target_os = "macos")]
    kqueue: i32,
    #[cfg(target_os = "macos")]
    kevents_buf: RefCell<Vec<kevent>>,
    // The following only need to be held on to, we don't touch them
    #[cfg(target_os = "macos")]
    _io_notification_port: *mut IONotificationPort,
    #[cfg(target_os = "macos")]
    _plug_notifications: io_object_t,
    #[cfg(target_os = "macos")]
    _unplug_notifications: io_object_t,
}
impl USBStubEngine {
    // XXX: Work around rustfmt not wanting to work otherwise??
    #[cfg(target_os = "macos")]
    fn init_real(
        mut this: pin_init::PinUninit<'_, Self>,
    ) -> pin_init::InitResult<'_, Self, Infallible> {
        let v = this.get_mut().as_mut_ptr();

        // Create kqueue fd
        let kq = unsafe { kqueue() };
        if kq < 0 {
            panic!(
                "failed to create kqueue {:?}",
                std::io::Error::last_os_error()
            );
        }

        // Set up a kevent for stdin
        let kevent_stdin = kevent::new(
            0,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
        );

        // Set up IOKit notifications incl. registering it for kevent
        let io_notif_port = unsafe { IONotificationPortCreate(0) };
        assert!(
            !io_notif_port.is_null(),
            "failed to create io notification port"
        );
        let io_notif_mach = unsafe { IONotificationPortGetMachPort(io_notif_port) };
        assert_ne!(
            io_notif_mach, 0,
            "failed to create io notification mach port"
        );

        let kevent_mach = kqueue_sys::kevent::new(
            io_notif_mach as usize,
            EventFilter::EVFILT_MACHPORT,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
        );

        let all_kevents = [kevent_stdin, kevent_mach];
        if unsafe {
            kqueue_sys::kevent(
                kq,
                all_kevents.as_ptr(),
                all_kevents.len() as i32,
                ptr::null_mut(),
                0,
                ptr::null(),
            )
        } < 0
        {
            panic!("kevent add failed: {:?}", std::io::Error::last_os_error());
        }

        // Prepare buffer for reading events
        let kevents_buf = Vec::with_capacity(all_kevents.len());

        // Register IOKit notifications
        let matching = unsafe { IOServiceMatching(b"IOUSBDevice\0".as_ptr()) };
        // SAFETY: We add two matching notifications, so bump refcount by 1
        unsafe { core_foundation::base::CFRetain(matching as *const _) };

        let mut plug_notifications = io_object_t(0);
        let ret = unsafe {
            IOServiceAddMatchingNotification(
                io_notif_port,
                b"IOServiceFirstMatch\0".as_ptr(),
                matching,
                Self::iokit_plug_cb,
                v as *const (),
                &mut plug_notifications,
            )
        };
        assert_eq!(ret, 0, "failed to watch USB hotplug notifications");

        let mut unplug_notifications = io_object_t(0);
        let ret = unsafe {
            IOServiceAddMatchingNotification(
                io_notif_port,
                b"IOServiceTerminate\0".as_ptr(),
                matching,
                Self::iokit_unplug_cb,
                v as *const (),
                &mut unplug_notifications,
            )
        };
        assert_eq!(ret, 0, "failed to watch USB hotplug unplug notifications");

        // SAFETY: Make sure we set everything
        unsafe {
            (*v).kqueue = kq;
            (*v).actual_needed_event_sz = Cell::new(all_kevents.len());
            // SAFETY: Don't drop uninit objects
            // (but others are okay, no drop impl)
            ptr::addr_of_mut!((*v).usb_devices).write(RefCell::new(HashMap::new()));
            ptr::addr_of_mut!((*v).kevents_buf).write(RefCell::new(kevents_buf));
            (*v)._io_notification_port = io_notif_port;
            (*v)._plug_notifications = plug_notifications;
            (*v)._unplug_notifications = unplug_notifications;
        }

        // Invoke iteration once to capture everything pre-existing
        Self::iokit_plug_cb(v as *const (), plug_notifications);
        Self::iokit_unplug_cb(v as *const (), unplug_notifications);

        // SAFETY: Make sure we set everything
        unsafe { Ok(this.init_ok()) }
    }

    #[cfg(target_os = "linux")]
    fn init_real(
        mut this: pin_init::PinUninit<'_, Self>,
    ) -> pin_init::InitResult<'_, Self, Infallible> {
        let v = this.get_mut().as_mut_ptr();

        // Create epoll fd
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            panic!(
                "failed to create epoll {:?}",
                std::io::Error::last_os_error()
            );
        }

        // Add stdin to epoll
        let mut epoll_stdin = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: RUNLOOP_EPOLL_STDIN,
        };
        let ret = unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, 0, &mut epoll_stdin) };
        if ret < 0 {
            panic!(
                "failed to epoll stdin {:?}",
                std::io::Error::last_os_error()
            );
        }

        // Set up connection to udev
        let udev = udev_sys::UdevNetlinkSocket::new().expect("failed to connect to udev");

        let mut epoll_udev = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: RUNLOOP_EPOLL_UDEV,
        };
        let ret =
            unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, udev.fd, &mut epoll_udev) };
        if ret < 0 {
            panic!(
                "failed to epoll udev socket {:?}",
                std::io::Error::last_os_error()
            );
        }

        // Prepare buffer for reading events
        let epoll_buf = Vec::with_capacity(2);

        // SAFETY: Make sure we set everything
        unsafe {
            (*v).epoll_fd = epoll_fd;
            (*v).actual_needed_event_sz = Cell::new(epoll_buf.len());
            (*v).next_session_id = Cell::new(0);
            (*v)._printed_caps_warning = Cell::new(false);
            // SAFETY: Don't drop uninit objects
            // (but others are okay, no drop impl)
            ptr::addr_of_mut!((*v).usb_devices).write(RefCell::new(HashMap::new()));
            ptr::addr_of_mut!((*v).epoll_buf).write(RefCell::new(epoll_buf));
            ptr::addr_of_mut!((*v).udev_connection).write(udev);
        }

        // SAFETY: Make sure we set everything
        unsafe { Ok(this.init_ok()) }
    }

    #[cfg(windows)]
    fn init_real(
        mut this: pin_init::PinUninit<'_, Self>,
    ) -> pin_init::InitResult<'_, Self, Infallible> {
        let v = this.get_mut().as_mut_ptr();

        let iocp = unsafe {
            windows_sys::Win32::System::IO::CreateIoCompletionPort(
                windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
                ptr::null_mut(),
                0,
                1,
            )
        };
        if iocp.is_null() {
            panic!("Couldn't create IOCP: {}", io::Error::last_os_error());
        }

        // SAFETY: Make sure we set everything
        unsafe {
            // SAFETY: Don't drop uninit objects
            // (but others are okay, no drop impl)
            ptr::addr_of_mut!((*v).usb_devices).write(RefCell::new(HashMap::new()));
            ptr::addr_of_mut!((*v).stdin_reader).write(WinStdinReader::new());
            WinNotificationHandler::new(ptr::addr_of_mut!((*v).notifcation_handler));
            (*v).iocp = iocp;
        }

        // SAFETY: Make sure we set everything
        unsafe { Ok(this.init_ok()) }
    }

    pub fn init() -> impl pin_init::Init<Self, Infallible> {
        pin_init::init_from_closure(Self::init_real)
    }

    fn handle_stdin(self: Pin<&Self>) -> bool {
        #[cfg(not(windows))]
        let msg = read_stdin_msg();
        #[cfg(windows)]
        let msg = self.stdin_reader.read_stdin_msg();
        if let Err(e) = &msg
            && e.kind() == io::ErrorKind::UnexpectedEof
        {
            log::debug!("EOF on stdin, goodbye!");
            return false;
        }
        let msg = msg.expect("failed to read stdin");

        let msg = str::from_utf8(&msg).expect("failed to parse message as utf-8");
        let msg_parsed: protocol::RequestMessage =
            serde_json::from_str(&msg).expect("failed to parse message as JSON");
        let txn_id = msg_parsed.txn_id().to_owned();

        match self.handle_message(msg_parsed) {
            Ok(DeviceOpResult::SendCompletionNow) => {
                let reply = protocol::ResponseMessage::RequestComplete {
                    txn_id,
                    babble: false,
                    data: None,
                    bytes_written: 0,
                };
                let reply = serde_json::to_string(&reply).unwrap();
                write_stdout_msg(reply.as_bytes()).expect("failed to write stdout");
            }
            Ok(DeviceOpResult::ManualCompletion) => {
                // Don't do anything right now
            }
            Err(err) => {
                let reply = protocol::ResponseMessage::RequestError {
                    txn_id,
                    error: err,
                    bytes_written: 0,
                };
                let reply = serde_json::to_string(&reply).unwrap();
                write_stdout_msg(reply.as_bytes()).expect("failed to write stdout");
            }
        }

        true
    }

    fn handle_message(self: Pin<&Self>, msg_parsed: protocol::RequestMessage) -> DeviceResult {
        match msg_parsed {
            protocol::RequestMessage::OpenDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.open_device(sid, &txn_id, self)
            }
            protocol::RequestMessage::CloseDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.close_device(sid, &txn_id)
            }
            protocol::RequestMessage::ResetDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.reset_device(sid, &txn_id)
            }
            protocol::RequestMessage::SetConfiguration { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.set_configuration(sid, &txn_id, value, self)
            }
            protocol::RequestMessage::ClaimInterface { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.claim_interface(sid, &txn_id, value)
            }
            protocol::RequestMessage::ReleaseInterface { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.release_interface(sid, &txn_id, value)
            }
            protocol::RequestMessage::SetAltInterface {
                sid,
                txn_id,
                iface,
                alt,
            } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.set_alt_interface(sid, &txn_id, iface, alt)
            }
            protocol::RequestMessage::ControlTransfer {
                sid,
                txn_id,
                request_type,
                request,
                value,
                index,
                data,
                length,
                _timeout_internal,
            } => {
                // Deal with data
                let mut txn_ok = false;
                let mut dir = USBTransferDirection::HostToDevice;
                let mut buf = Vec::new();
                let mut len = 0;
                if request_type & usb_ch9::ch9_core::REQ_DIR_D2H != 0 {
                    if length.is_some() && data.is_none() {
                        txn_ok = true;
                        dir = USBTransferDirection::DeviceToHost;
                        len = length.unwrap() as usize;
                        #[cfg(not(target_os = "linux"))]
                        {
                            // As a huge hack, on Linux we later allocate a new buffer
                            // (so, don't waste memory allocating this buffer here)
                            buf = Vec::with_capacity(len);
                        }
                    }
                } else {
                    if data.is_some() && length.is_none() {
                        txn_ok = true;
                        dir = USBTransferDirection::HostToDevice;
                        buf = URL_SAFE_NO_PAD
                            .decode(&data.unwrap())
                            .expect("base64 decode error");
                        len = buf.len();
                    }
                }
                assert!(txn_ok, "received malformed request");
                // Deal with data size limits
                if len > u16::MAX as usize {
                    return Err(protocol::Errors::RequestTooBig);
                }

                // timeout of 0 --> no timeout
                let timeout = _timeout_internal.unwrap_or_default();

                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.ctrl_xfer(
                    sid,
                    &txn_id,
                    dir,
                    request_type,
                    request,
                    value,
                    index,
                    len as u16,
                    buf,
                    timeout,
                    self,
                )
            }
            protocol::RequestMessage::DataTransfer {
                sid,
                txn_id,
                ep,
                data,
                length,
            } => {
                // Deal with data
                let mut txn_ok = false;
                let mut dir = USBTransferDirection::HostToDevice;
                let mut buf = Vec::new();
                let mut len = 0;
                if ep & usb_ch9::ch9_core::EP_DIR_IN != 0 {
                    if length.is_some() && data.is_none() {
                        txn_ok = true;
                        dir = USBTransferDirection::DeviceToHost;
                        len = length.unwrap() as usize;
                        buf = Vec::with_capacity(len);
                    }
                } else {
                    if data.is_some() && length.is_none() {
                        txn_ok = true;
                        dir = USBTransferDirection::HostToDevice;
                        buf = URL_SAFE_NO_PAD
                            .decode(&data.unwrap())
                            .expect("base64 decode error");
                        len = buf.len();
                    }
                }
                assert!(txn_ok, "received malformed request");
                // Deal with data size limits
                if len > u32::MAX as usize {
                    return Err(protocol::Errors::RequestTooBig);
                }

                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.data_xfer(sid, &txn_id, dir, ep, len as u32, buf)
            }
            protocol::RequestMessage::ClearHalt { sid, txn_id, ep } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.clear_halt(sid, &txn_id, ep)
            }
            protocol::RequestMessage::IsocTransfer {
                sid,
                txn_id,
                ep,
                data,
                pkt_len,
            } => {
                // Deal with data size limits
                let mut total_len = 0;
                let num_pkts = pkt_len.len();
                #[cfg(target_os = "macos")]
                if num_pkts > u32::MAX as usize {
                    return Err(protocol::Errors::RequestTooBig);
                }
                #[cfg(target_os = "linux")]
                if num_pkts > i32::MAX as usize {
                    return Err(protocol::Errors::RequestTooBig);
                }
                for l in &pkt_len {
                    if *l > u16::MAX as u32 {
                        return Err(protocol::Errors::RequestTooBig);
                    }
                    total_len += *l as usize;
                }

                // Deal with data
                let mut txn_ok = false;
                let mut dir = USBTransferDirection::HostToDevice;
                let mut buf = Vec::new();
                if ep & usb_ch9::ch9_core::EP_DIR_IN != 0 {
                    if data.is_none() {
                        txn_ok = true;
                        dir = USBTransferDirection::DeviceToHost;
                        buf = Vec::with_capacity(total_len);
                    }
                } else {
                    if data.is_some() {
                        txn_ok = true;
                        dir = USBTransferDirection::HostToDevice;
                        buf = URL_SAFE_NO_PAD
                            .decode(&data.unwrap())
                            .expect("base64 decode error");
                        if buf.len() != total_len {
                            log::warn!("Wrong buffer size, sid = {}, txn = {}", sid, txn_id);
                            buf.resize(total_len, 0);
                        }
                    }
                }
                assert!(txn_ok, "received malformed request");

                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                let usb_dev = devices
                    .get_mut(&sid)
                    .ok_or(protocol::Errors::DeviceNotFound)?;

                usb_dev.isoc_xfer(sid, &txn_id, dir, ep, total_len, pkt_len, buf)
            }
        }
    }

    #[cfg(target_os = "macos")]
    /// Run one loop. Returns true if we should continue
    pub fn run_loop(self: Pin<&Self>) -> bool {
        // Poll for events
        let mut kevents_buf = self.kevents_buf.borrow_mut();

        // If we need to, grow the event buffer.
        // We have to do the grow here, and *NOT* immediately when adding new things to watch,
        // because new items get added while we are still iterating over the buffer.
        let needed_events = self.actual_needed_event_sz.get();
        if kevents_buf.capacity() < needed_events {
            let to_reserve = needed_events - kevents_buf.len();
            kevents_buf.reserve(to_reserve);
        }

        unsafe {
            let nevents = kqueue_sys::kevent(
                self.kqueue,
                ptr::null(),
                0,
                kevents_buf.as_mut_ptr(),
                kevents_buf.capacity() as i32,
                ptr::null(),
            );

            if nevents < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    return true;
                }
                panic!("kevent poll failed: {:?}", err);
            }

            // SAFETY: Set length to actual number of events
            kevents_buf.set_len(nevents as usize);
        };

        for kevent in kevents_buf.iter() {
            if kevent.ident == 0 && kevent.filter == EventFilter::EVFILT_READ {
                let cont = self.handle_stdin();
                if !cont {
                    return false;
                }
            } else if kevent.filter == EventFilter::EVFILT_MACHPORT {
                let mut msg = OpaqueMachMessage::default();
                let ret = unsafe {
                    mach_msg(
                        &mut msg as *mut _,
                        MACH_RCV_MSG,
                        0,
                        mem::size_of::<OpaqueMachMessage>() as u32,
                        kevent.ident as u32,
                        0,
                        0,
                    )
                };
                // Ignore MACH_RCV_INVALID_NAME
                // It probably just means that we tried to process a lingering callbacks
                // for a device which was unplugged and that we already closed.
                if ret != 0 && ret != MACH_RCV_INVALID_NAME {
                    log::warn!("mach_msg receive failed {:08x}", ret as u32);
                    continue;
                }

                unsafe {
                    // SAFETY: This ends up calling the callbacks,
                    // which require us to have set everything up perfectly.
                    IODispatchCalloutFromMessage(
                        ptr::null(),
                        &msg as *const _,
                        self._io_notification_port as *const (),
                    );
                }
            } else {
                log::warn!("Unknown kqueue event {:?}", kevent);
            }
        }

        true
    }

    #[cfg(target_os = "linux")]
    /// Run one loop. Returns true if we should continue
    pub fn run_loop(self: Pin<&Self>) -> bool {
        // Poll for events
        let mut epoll_buf = self.epoll_buf.borrow_mut();

        // If we need to, grow the event buffer.
        // We have to do the grow here, and *NOT* immediately when adding new things to watch,
        // because new items get added while we are still iterating over the buffer.
        let needed_events = self.actual_needed_event_sz.get();
        if epoll_buf.capacity() < needed_events {
            let to_reserve = needed_events - epoll_buf.len();
            epoll_buf.reserve(to_reserve);
        }

        unsafe {
            let nevents = libc::epoll_wait(
                self.epoll_fd,
                epoll_buf.as_mut_ptr(),
                epoll_buf.capacity() as i32,
                -1,
            );

            if nevents < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    return true;
                }
                panic!("epoll failed: {:?}", err);
            }

            // SAFETY: Set length to actual number of events
            epoll_buf.set_len(nevents as usize);
        };

        for evt in epoll_buf.iter() {
            match evt.u64 {
                RUNLOOP_EPOLL_STDIN => {
                    if evt.events & (libc::EPOLLIN as u32) != 0 {
                        let cont = self.handle_stdin();
                        if !cont {
                            return false;
                        }
                    }
                    if evt.events & (libc::EPOLLHUP as u32) != 0 {
                        log::debug!("HUP on stdin, goodbye!");
                        return false;
                    }
                }
                RUNLOOP_EPOLL_UDEV => loop {
                    match self.udev_connection.get_event() {
                        Ok(udev_evt) => {
                            if let Some(udev_evt) = udev_evt {
                                let dev_usb_path = if let Some(Some(devname)) =
                                    udev_evt.get(b"DEVNAME".as_slice())
                                {
                                    Path::new(OsStr::from_bytes(devname))
                                } else {
                                    log::warn!("udev event without DEVNAME");
                                    break;
                                };

                                let mut sysfs_path = PathBuf::new();
                                sysfs_path.push("/sys");
                                if let Some(Some(devpath)) = udev_evt.get(b"DEVPATH".as_slice()) {
                                    sysfs_path.push(if devpath.starts_with(b"/") {
                                        OsStr::from_bytes(&devpath[1..])
                                    } else {
                                        OsStr::from_bytes(devpath)
                                    });
                                } else {
                                    log::warn!("udev event without DEVPATH");
                                    break;
                                };

                                log::debug!(
                                    "plug, {} sysfs {}",
                                    dev_usb_path.to_string_lossy(),
                                    sysfs_path.to_string_lossy()
                                );

                                if let Err(err) =
                                    USBDevice::setup(dev_usb_path, Some(&sysfs_path), self)
                                {
                                    log::warn!(
                                        "Device setup failed! path = {:?}, err = {}",
                                        dev_usb_path,
                                        err
                                    );
                                }
                            } else {
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("udev event reading failed {:?}", e);
                        }
                    }
                },
                _ if evt.u64 >= 0x80000000_00000000 => {
                    if evt.events & (libc::EPOLLIN as u32) != 0 {
                        // This indicates a timeout

                        let sessionid = evt.u64 & 0x7fffffff_ffffffff;
                        log::debug!("timeout sid {}", sessionid);

                        let mut devices = self.usb_devices.borrow_mut();
                        let dev = devices.get_mut(&sessionid);
                        if dev.is_none() {
                            log::warn!("Timeout on a sessionID we don't have?? 0x{:x}", sessionid);
                            // Make sure we don't ignore other events on this FD
                            continue;
                        }
                        let dev = dev.unwrap();
                        let fd = dev._linux_handles.dev_fd;

                        if let Some(urb) = dev._linux_timeout_urb {
                            let ret = unsafe { libc::ioctl(fd, libc::_IO(b'U' as u32, 11), urb) };
                            if ret < 0 {
                                log::warn!(
                                    "Discarding URB failed, sid 0x{:x} urb {:?}",
                                    sessionid,
                                    urb
                                );
                            } else {
                                dev._linux_timeout_urb = None;
                            }
                        }
                    }
                }
                _ if evt.u64 < 0x80000000_00000000 => {
                    'usb_completion: {
                        if evt.events & (libc::EPOLLOUT as u32) != 0 {
                            // Some form of completion

                            let sessionid = evt.u64;
                            log::debug!("reap sid {}", sessionid);

                            let mut devices = self.usb_devices.borrow_mut();
                            let dev = devices.get_mut(&sessionid);
                            if dev.is_none() {
                                log::warn!("Reaping a sessionID we don't have?? 0x{:x}", sessionid);
                                // Make sure we don't ignore other events on this FD
                                break 'usb_completion;
                            }
                            let dev = dev.unwrap();
                            let fd = dev._linux_handles.dev_fd;

                            loop {
                                let mut urb: *mut usbdevfs_urb = ptr::null_mut();
                                let ret = unsafe {
                                    libc::ioctl(
                                        fd,
                                        libc::_IOW::<*mut ()>(b'U' as u32, 13),
                                        &mut urb,
                                    )
                                };
                                if ret != 0 {
                                    let err = io::Error::last_os_error();
                                    let errno = err.raw_os_error().unwrap();
                                    // These two errors are expected once we run out of URBs to reap
                                    if errno != libc::EAGAIN && errno != libc::ENODEV {
                                        log::warn!("URB reap failed! {}", err);
                                    }
                                    break;
                                }

                                if dev._linux_timeout_urb == Some(urb) {
                                    dev._linux_timeout_urb = None;
                                }

                                let is_iso = unsafe { (*urb).type_ == USBDEVFS_URB_TYPE_ISO };
                                if !is_iso {
                                    let urb = unsafe {
                                        let wrapped_ptr =
                                            (*urb).usercontext as *mut LinuxURBWrapper;
                                        // SAFETY: Need to make sure this matches up with how we queue URBs
                                        let mut ret = Box::from_raw(wrapped_ptr);
                                        if ret.urb.type_ == USBDEVFS_URB_TYPE_CONTROL {
                                            // Linux reports the control transfer length, but doesn't include/remove the setup packet
                                            ret.buf.set_len(8 + ret.urb.actual_length as usize);
                                        } else {
                                            ret.buf.set_len(ret.urb.actual_length as usize);
                                        }
                                        ret
                                    };

                                    urb.notify_completion();
                                } else {
                                    let urb = unsafe {
                                        let wrapped_ptr =
                                            (*urb).usercontext as *mut LinuxIsoURBWrapper;
                                        // SAFETY: Need to make sure this matches up with how we queue URBs
                                        let mut ret = Box::from_raw(wrapped_ptr);
                                        ret.buf.set_len(ret.urb.urb.actual_length as usize);
                                        ret
                                    };

                                    urb.notify_completion();
                                }
                            }
                        }
                    }
                    if evt.events & (libc::EPOLLHUP as u32) != 0 {
                        // Device is unplugged
                        let sessionid = evt.u64;
                        log::debug!("unplug sid {}", sessionid);

                        let devices = self.usb_devices.borrow();
                        let dev = devices.get(&sessionid);
                        if dev.is_none() {
                            log::warn!("Removing a sessionID we don't have?? 0x{:x}", sessionid);
                        }

                        // Send notification
                        let notif = protocol::ResponseMessage::UnplugDevice {
                            sid: sessionid.to_string(),
                        };
                        let notif = serde_json::to_string(&notif).unwrap();
                        write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
                    }
                    if evt.events & (libc::EPOLLERR as u32) != 0 {
                        // Device is fully gone, all URBs reaped
                        let sessionid = evt.u64;
                        log::debug!("destroy sid {}", sessionid);

                        let mut devices = self.usb_devices.borrow_mut();
                        let dev = devices.remove(&sessionid);
                        if dev.is_none() {
                            log::warn!("Destroying a sessionID we don't have?? 0x{:x}", sessionid);
                        }
                        drop(dev);
                    }
                }
                _ => unreachable!(),
            }
        }

        true
    }

    /// Start watching a new usbdevfs fd
    ///
    /// Implicitly incrememnts actual_needed_event_sz
    #[cfg(target_os = "linux")]
    pub(crate) fn add_usb_fd(&self, fd: i32, sid: u64) -> io::Result<()> {
        // Register the port in epoll
        let mut epoll_usb = libc::epoll_event {
            events: libc::EPOLLOUT as u32,
            u64: sid,
        };
        let ret =
            unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut epoll_usb) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // Bump the buffer size
        self.actual_needed_event_sz.update(|x| x + 1);

        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub fn new_sid(&self) -> u64 {
        let sessionid = self.next_session_id.get();
        self.next_session_id.set(sessionid + 1);

        if sessionid >= (0x80000000_00000000) {
            panic!("Too many session IDs! (how???)");
        }

        sessionid
    }

    #[cfg(target_os = "macos")]
    extern "C" fn iokit_plug_cb(self_: *const (), iterator: io_object_t) {
        // SAFETY: We passed in self as the arg, and we init as pinned
        let self_ = unsafe { Pin::new_unchecked(&*(self_ as *const Self)) };

        let mut item;
        loop {
            item = unsafe { IOIteratorNext(iterator) };
            if item.0 == 0 {
                break;
            }

            let sessionid = get_session_id(item);
            if let Some(sessionid) = sessionid {
                log::debug!("plug, session = 0x{:x}", sessionid);

                if let Err(err) = USBDevice::setup(item, sessionid, self_) {
                    log::warn!(
                        "Device setup failed! session = 0x{:x}, err = 0x{:08x}",
                        sessionid,
                        err as u32
                    );
                }
            } else {
                log::warn!("Got plug notification without a sessionID??");
                unsafe { IOObjectRelease(item) };
            }
        }
    }

    #[cfg(target_os = "macos")]
    extern "C" fn iokit_unplug_cb(self_: *const (), iterator: io_object_t) {
        // SAFETY: We passed in self as the arg, and we init as pinned
        let self_ = unsafe { Pin::new_unchecked(&*(self_ as *const Self)) };

        let mut item;
        loop {
            item = unsafe { IOIteratorNext(iterator) };
            if item.0 == 0 {
                break;
            }

            let sessionid = get_session_id(item);
            if let Some(sessionid) = sessionid {
                log::debug!("unplug, session = 0x{:x}", sessionid);

                let mut devices = self_.usb_devices.borrow_mut();
                let dev = devices.remove(&sessionid);
                if dev.is_none() {
                    log::warn!("Removing a sessionID we don't have?? 0x{:x}", sessionid);
                }
                drop(dev);

                // Send notification
                let notif = protocol::ResponseMessage::UnplugDevice {
                    sid: sessionid.to_string(),
                };
                let notif = serde_json::to_string(&notif).unwrap();
                write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
            } else {
                log::warn!("Got unplug notification without a sessionID??");
            }

            let ret = unsafe { IOObjectRelease(item) };
            assert_eq!(ret, 0);
        }
    }

    /// Start watching a new Mach port
    ///
    /// Implicitly incrememnts actual_needed_event_sz
    #[cfg(target_os = "macos")]
    pub(crate) fn add_mach_port(&self, mach_port: libc::mach_port_t) -> io::Result<()> {
        // Register the port in kqueue
        let kevent_new = kqueue_sys::kevent::new(
            mach_port as usize,
            kqueue_sys::EventFilter::EVFILT_MACHPORT,
            kqueue_sys::EventFlag::EV_ADD,
            kqueue_sys::FilterFlag::empty(),
        );
        let ret = unsafe {
            kqueue_sys::kevent(self.kqueue, &kevent_new, 1, ptr::null_mut(), 0, ptr::null())
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        // Bump the buffer size
        self.actual_needed_event_sz.update(|x| x + 1);

        Ok(())
    }

    #[cfg(windows)]
    /// Run one loop. Returns true if we should continue
    pub fn run_loop(self: Pin<&Self>) -> bool {
        log::debug!("win32 loop");

        // Poll for events
        let waits = [self.stdin_reader.event, self.notifcation_handler.h_event];

        let wait_result =
            unsafe { WaitForMultipleObjectsEx(waits.len() as u32, waits.as_ptr(), 0, INFINITE, 1) };

        match wait_result {
            0 => {
                // FIXME: This peek mechanism is a hack
                while self.stdin_reader.peek_stdin() {
                    let cont = self.handle_stdin();
                    if !cont {
                        return false;
                    }
                }
            }
            1 => {
                while let Some(hotplug_notif) = self.notifcation_handler.get_notif() {
                    match &hotplug_notif {
                        WinHotplugNotification::NewInterface { .. } => {
                            USBDevice::setup(hotplug_notif, self);
                        }
                        WinHotplugNotification::RemoveInterface {
                            session_id,
                            interface_no,
                            interface_path: _,
                        } => {
                            let mut devices = self.usb_devices.borrow_mut();
                            if let hash_map::Entry::Occupied(mut dev) = devices.entry(*session_id) {
                                let removed = dev.get_mut()._win_iface_paths.remove(interface_no);
                                if removed.is_none() {
                                    log::warn!(
                                        "Removing an interface {:02x} of sessionID we don't have?? 0x{:x}",
                                        interface_no,
                                        session_id
                                    );
                                }

                                if dev.get()._win_iface_paths.len() == 0 {
                                    // Send notification
                                    let notif = protocol::ResponseMessage::UnplugDevice {
                                        sid: session_id.to_string(),
                                    };
                                    let notif = serde_json::to_string(&notif).unwrap();
                                    write_stdout_msg(notif.as_bytes())
                                        .expect("failed to write stdout");

                                    dev.remove();
                                }
                            } else {
                                log::warn!(
                                    "Removing a sessionID we don't have?? 0x{:x}",
                                    session_id
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        true
    }
}
impl Drop for USBStubEngine {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            IOObjectRelease(self._plug_notifications);
            IOObjectRelease(self._unplug_notifications);
            IONotificationPortDestroy(self._io_notification_port);
            libc::close(self.kqueue);
        }
        #[cfg(target_os = "linux")]
        unsafe {
            libc::close(self.epoll_fd);
        }
        #[cfg(windows)]
        unsafe {
            // Post a quit notification to the IOCP thread
            windows_sys::Win32::System::IO::PostQueuedCompletionStatus(
                self.iocp,
                0,
                1,
                ptr::null(),
            );
            windows_sys::Win32::Foundation::CloseHandle(self.iocp);
        }
    }
}

fn main() {
    stderrlog::new()
        .verbosity(log::Level::Debug)
        .init()
        .unwrap();
    log::info!("awawausb stub starting!");

    pin_init::init_stack!(state = USBStubEngine::init());
    let state = state.unwrap();
    // SAFETY: We need to *immediately* get rid of &mut,
    // because OS callbacks etc hold a reference to the engine object.
    // This is probably *still* incorrect, but dunno how to fix.
    let state = state.as_ref();

    #[cfg(windows)]
    {
        match state.notifcation_handler.probe_existing() {
            Ok(probe_results) => {
                for probe_result in probe_results {
                    match probe_result {
                        WinHotplugNotification::NewInterface { .. } => {
                            USBDevice::setup(probe_result, state)
                        }
                        WinHotplugNotification::RemoveInterface { .. } => {
                            log::warn!("WTF? Got a device removal during initial probe");
                        }
                    }
                }
            }
            Err(err) => log::warn!("Initial probing device failed! {}", err),
        }

        let iocp = state.iocp.addr();
        std::thread::spawn(move || iocp_thread(iocp));
    }

    #[cfg(target_os = "linux")]
    // Now we have to do *this stupid bullshit* to find existing USB devices
    'find_existing_devs: {
        let dir_usbroot = unsafe { libc::opendir(c"/dev/bus/usb".as_ptr()) };
        if dir_usbroot.is_null() {
            break 'find_existing_devs;
        }

        loop {
            let dirent_bus = unsafe { libc::readdir(dir_usbroot) };
            if dirent_bus.is_null() {
                break;
            }
            let dirent_bus = unsafe { *dirent_bus };

            if dirent_bus.d_type == libc::DT_DIR {
                let dirname_bus =
                    CStr::from_bytes_until_nul(unfuck_bytes(&dirent_bus.d_name)).unwrap();
                if dirname_bus != c"." && dirname_bus != c".." {
                    let mut path_bus = b"/dev/bus/usb/".to_vec();
                    path_bus.extend_from_slice(dirname_bus.to_bytes());
                    path_bus.push(0);

                    let dir_bus = unsafe {
                        libc::opendir(CString::from_vec_with_nul(path_bus).unwrap().as_ptr())
                    };
                    if dir_bus.is_null() {
                        continue;
                    }

                    loop {
                        let dirent_dev = unsafe { libc::readdir(dir_bus) };
                        if dirent_dev.is_null() {
                            break;
                        }
                        let dirent_dev = unsafe { *dirent_dev };

                        if dirent_dev.d_type == libc::DT_CHR {
                            let dirname_dev =
                                CStr::from_bytes_until_nul(unfuck_bytes(&dirent_dev.d_name))
                                    .unwrap();

                            let mut full_path = PathBuf::new();
                            full_path.push("/dev/bus/usb");
                            full_path.push(OsStr::from_bytes(dirname_bus.to_bytes()));
                            full_path.push(OsStr::from_bytes(dirname_dev.to_bytes()));

                            log::debug!("inital probing, path = {:?}", full_path);

                            if let Err(err) = USBDevice::setup(&full_path, None, state) {
                                log::warn!(
                                    "Device setup failed! path = {:?}, err = {}",
                                    full_path,
                                    err
                                );
                            }
                        }
                    }

                    unsafe { libc::closedir(dir_bus) };
                }
            }
        }

        unsafe { libc::closedir(dir_usbroot) };
    }

    while state.run_loop() {}

    log::info!("awawausb stub exiting!");
}
