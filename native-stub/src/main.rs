use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io;
use std::mem;
use std::pin::Pin;
use std::ptr;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use core_foundation::base::CFRetain;
use kqueue_sys::*;

mod macos_sys;
mod macos_wrap;
pub mod protocol;
mod stdio_unix;

use macos_sys::*;
use macos_wrap::*;
use stdio_unix::*;

use crate::protocol::DeviceConfiguration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum USBTransferDirection {
    HostToDevice,
    DeviceToHost,
}

/// A USB transfer which is currently in-flight
///
/// This would usually be called an "URB" (USB Request Block).
///
/// When the transfer has been submitted, this struct is "owned by"
/// the operating system. We reclaim ownership when we get a completion.
#[derive(Debug)]
pub struct USBTransfer {
    pub dir: USBTransferDirection,
    pub txn_id: String,
    /// The payload buffer
    ///
    /// If sending data _to_ the device, this buffer contains the data.
    /// If receiving data _from_ the device, this buffer must have
    /// a capacity large enough to hold the desired length.
    ///
    /// The kernel _may write_ to this buffer during the time we've
    /// given up ownership
    pub buf: Vec<u8>,
}

/// State regarding each interface
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct USBInterfaceState {
    pub alt_setting: u8,
    pub claimed: bool,

    _macos_iface_idx: usize,
    /// For (reverse) mapping from endpoint address to pipeRef
    _macos_ep_addrs: Vec<u8>,
}

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

    pub reformatted_config_descriptors: Vec<DeviceConfiguration>,

    pub opened: bool,
    pub current_configuration_id: u8,
    /// Map from bInterfaceNumber to state
    pub current_if_state: HashMap<u8, USBInterfaceState>,

    /// Map from endpoint address to (interface index, pipeRef)
    ///
    /// (macOS specific)
    _ep_to_idx: HashMap<u8, (usize, u8)>,
    _macos_dev: IOUSBDeviceStruct,
    _macos_ifaces: Vec<IOUSBInterfaceStruct>,
}
impl USBDevice {
    #[allow(non_snake_case)]
    pub fn setup(obj: io_object_t, engine: Pin<&USBStubEngine>) -> Option<Self> {
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
        let mach_port = usb_dev.CreateDeviceAsyncPort().ok()?;
        engine.add_mach_port(mach_port).ok()?;
        usb_dev.1 = &engine.actual_needed_event_sz;

        // Get device descriptor fields
        let bDeviceClass = usb_dev.GetDeviceClass().ok()?;
        let bDeviceSubClass = usb_dev.GetDeviceSubClass().ok()?;
        let bDeviceProtocol = usb_dev.GetDeviceProtocol().ok()?;
        let idVendor = usb_dev.GetDeviceVendor().ok()?;
        let idProduct = usb_dev.GetDeviceProduct().ok()?;
        let bcdDevice = usb_dev.GetDeviceReleaseNumber().ok()?;
        let iManufacturer = usb_dev.USBGetManufacturerStringIndex().ok()?;
        let iProduct = usb_dev.USBGetProductStringIndex().ok()?;
        let iSerialNumber = usb_dev.USBGetSerialNumberStringIndex().ok()?;
        let bNumConfigurations = usb_dev.GetNumberOfConfigurations().ok()?;

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
            let conf_desc = usb_dev.GetConfigurationDescriptorPtr(i).ok()?;

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
        let current_config = usb_dev.GetConfiguration().ok()?;

        let mut dev = USBDevice {
            device_descriptor: dev_desc,
            config_descriptors: config_descs,
            vendor_name: str_manuf,
            product_name: str_product,
            serial_number: str_sn,

            reformatted_config_descriptors: Vec::new(),

            opened: false,
            current_configuration_id: current_config,
            current_if_state: HashMap::new(),

            _ep_to_idx: HashMap::new(),
            _macos_dev: usb_dev,
            _macos_ifaces: Vec::new(),
        };

        // Open a handle to all the interfaces
        dev.macos_probe_ifaces().ok()?;
        dev.reformat_config_descriptors();

        Some(dev)
    }

    pub(crate) fn reformat_config_descriptors(&mut self) {
        let mut configs = Vec::new();
        for cfg_desc in &self.config_descriptors {
            let mut this_config_desc = None;
            for desc in usb_ch9::parse_descriptor_set(&cfg_desc) {
                match desc {
                    usb_ch9::DescriptorRef::Config(d) => {
                        if this_config_desc.is_some() {
                            log::warn!("Bogus extra configuration descriptor? {:?}", desc)
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
                                    self.current_if_state.get(&d.bInterfaceNumber);
                                match current_if_state {
                                    Some(x) => x.alt_setting,
                                    None => {
                                        log::warn!(
                                            "Descriptor has an interface 0x{:02x} we don't know about",
                                            d.bInterfaceNumber
                                        );
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
    }

    /// Open or re-open all interface handles, when switching configuration
    pub(crate) fn macos_probe_ifaces(&mut self) -> Result<(), libc::kern_return_t> {
        // Make sure we close all the existing handles first
        self._macos_ifaces.clear();

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

            ifaces.push(usb_iface);
            iface_idx += 1;
        }
        unsafe { IOObjectRelease(iter_ifaces) };

        self.current_if_state = if_states;
        self._macos_ifaces = ifaces;

        Ok(())
    }
}

/// Main struct holding all of the state for our operations
#[derive(Debug)]
pub struct USBStubEngine {
    usb_devices: RefCell<HashMap<u64, USBDevice>>,

    // As we watch more things, we make the event buffer bigger.
    // But we never make it smaller, so this field keeps track of
    // how many events we _actually_ need.
    actual_needed_event_sz: Cell<usize>,
    kqueue: i32,
    kevents_buf: RefCell<Vec<kevent>>,
    // The following only need to be held on to, we don't touch them
    _io_notification_port: *mut IONotificationPort,
    _plug_notifications: io_object_t,
    _unplug_notifications: io_object_t,
}
impl USBStubEngine {
    // XXX: Work around rustfmt not wanting to work otherwise??
    fn init_real(
        mut this: pin_init::PinUninit<'_, Self>,
    ) -> pin_init::InitResult<'_, Self, Infallible> {
        let v = this.get_mut().as_mut_ptr();

        // Create kqueue fd
        let kq = unsafe { kqueue() };

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
        unsafe { CFRetain(matching as *const _) };

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

    pub fn init() -> impl pin_init::Init<Self, Infallible> {
        pin_init::init_from_closure(Self::init_real)
    }

    fn handle_stdin(self: Pin<&Self>) -> bool {
        let msg = read_stdin_msg();
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

        macro_rules! send_error {
            ($txn_id:expr, $err:ident) => {
                let reply = protocol::ResponseMessage::RequestError {
                    txn_id: $txn_id,
                    error: protocol::Errors::$err,
                    bytes_written: 0,
                };
                let reply = serde_json::to_string(&reply).unwrap();
                write_stdout_msg(reply.as_bytes()).expect("failed to write stdout");
            };
        }

        macro_rules! send_completion {
            ($txn_id:expr) => {
                let reply = protocol::ResponseMessage::RequestComplete {
                    txn_id: $txn_id,
                    babble: false,
                    data: None,
                    bytes_written: 0,
                };
                let reply = serde_json::to_string(&reply).unwrap();
                write_stdout_msg(reply.as_bytes()).expect("failed to write stdout");
            };
        }

        match msg_parsed {
            protocol::RequestMessage::EchoTest { msg } => {
                let reply = protocol::ResponseMessage::EchoResponse { msg };
                let reply = serde_json::to_string(&reply).unwrap();
                write_stdout_msg(reply.as_bytes()).expect("failed to write stdout");
            }
            protocol::RequestMessage::OpenDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if usb_dev.opened {
                        log::debug!(
                            "Opening already opened device, sid = {}, txn = {}",
                            sid,
                            txn_id
                        );
                        send_completion!(txn_id);
                    } else {
                        log::debug!("device open, sid = {}, txn = {}", sid, txn_id);
                        if let Err(ret) = usb_dev._macos_dev.USBDeviceOpen() {
                            log::warn!(
                                "USBDeviceOpen failed, sid = {}, txn = {}, ret = {:08x} ",
                                sid,
                                txn_id,
                                ret
                            );
                            if ret == kIOReturnExclusiveAccess {
                                send_error!(txn_id, AlreadyClaimed);
                            } else {
                                send_error!(txn_id, TransferError);
                            }
                        } else {
                            // Open successful
                            usb_dev.opened = true;
                            send_completion!(txn_id);
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
            protocol::RequestMessage::CloseDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if !usb_dev.opened {
                        send_error!(txn_id, InvalidState);
                    } else {
                        log::debug!("device close, sid = {}, txn = {}", sid, txn_id);

                        // Release all interfaces before closing
                        for (_, iface_state) in usb_dev.current_if_state.iter_mut() {
                            if iface_state.claimed {
                                log::debug!(
                                    " -> releasing interface {}",
                                    iface_state._macos_iface_idx
                                );

                                if let Err(ret) = usb_dev._macos_ifaces
                                    [iface_state._macos_iface_idx]
                                    .USBInterfaceClose()
                                {
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

                        if let Err(ret) = usb_dev._macos_dev.USBDeviceClose() {
                            log::warn!(
                                "USBDeviceClose failed, sid = {}, txn = {}, ret = {:08x} ",
                                sid,
                                txn_id,
                                ret
                            );
                            send_error!(txn_id, TransferError);
                        } else {
                            // Close successful
                            usb_dev.opened = false;
                            send_completion!(txn_id);
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
            protocol::RequestMessage::ResetDevice { sid, txn_id } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if !usb_dev.opened {
                        send_error!(txn_id, InvalidState);
                    } else {
                        log::debug!("device reset, sid = {}, txn = {}", sid, txn_id);
                        if let Err(ret) = usb_dev._macos_dev.USBDeviceReEnumerate(0) {
                            log::warn!(
                                "USBDeviceReEnumerate failed, sid = {}, txn = {}, ret = {:08x} ",
                                sid,
                                txn_id,
                                ret
                            );
                            send_error!(txn_id, TransferError);
                        } else {
                            // Reset successful
                            send_completion!(txn_id);
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
            protocol::RequestMessage::SetConfiguration { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if !usb_dev.opened {
                        send_error!(txn_id, InvalidState);
                    } else {
                        log::debug!(
                            "device set config 0x{:02x}, sid = {}, txn = {}",
                            value,
                            sid,
                            txn_id
                        );
                        if let Err(ret) = usb_dev._macos_dev.SetConfiguration(value) {
                            log::warn!(
                                "SetConfiguration failed, sid = {}, txn = {}, ret = {:08x} ",
                                sid,
                                txn_id,
                                ret
                            );
                            send_error!(txn_id, TransferError);
                        } else {
                            // Set config successful
                            usb_dev.current_configuration_id = value;
                            if let Err(ret) = usb_dev.macos_probe_ifaces() {
                                log::warn!(
                                    "SetConfiguration failed reopening interfaces, sid = {}, txn = {}, ret = {:08x} ",
                                    sid,
                                    txn_id,
                                    ret
                                );
                                send_error!(txn_id, TransferError);
                            } else {
                                send_completion!(txn_id);
                            }
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
            protocol::RequestMessage::ClaimInterface { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if !usb_dev.opened {
                        send_error!(txn_id, InvalidState);
                    } else {
                        if let Some(iface_state) = usb_dev.current_if_state.get_mut(&value) {
                            if iface_state.claimed {
                                log::debug!(
                                    "Claiming already claimed interface 0x{:02x}, sid = {}, txn = {}",
                                    value,
                                    sid,
                                    txn_id
                                );
                                send_completion!(txn_id);
                            } else {
                                log::debug!(
                                    "device claim interface 0x{:02x}, sid = {}, txn = {}",
                                    value,
                                    sid,
                                    txn_id
                                );

                                let mac_iface_obj =
                                    &mut usb_dev._macos_ifaces[iface_state._macos_iface_idx];
                                if let Err(ret) = mac_iface_obj.USBInterfaceOpen() {
                                    log::warn!(
                                        "USBInterfaceOpen failed {}, sid = {}, txn = {}, ret = {:08x} ",
                                        iface_state._macos_iface_idx,
                                        sid,
                                        txn_id,
                                        ret
                                    );
                                    if ret == kIOReturnExclusiveAccess {
                                        send_error!(txn_id, AlreadyClaimed);
                                    } else {
                                        send_error!(txn_id, TransferError);
                                    }
                                } else {
                                    // Claim interface successful
                                    iface_state.claimed = true;

                                    // Update this !@#$ list
                                    assert_eq!(iface_state._macos_ep_addrs.len(), 0);
                                    let ep_nums = mac_iface_obj.get_ep_addrs();
                                    for (piperef_m1, ep) in ep_nums.iter().enumerate() {
                                        let old = usb_dev._ep_to_idx.insert(
                                            *ep,
                                            (iface_state._macos_iface_idx, (piperef_m1 + 1) as u8),
                                        );
                                        if old.is_some() {
                                            log::warn!("Duplicate endpoint?! {:02x}", ep);
                                        }
                                    }
                                    iface_state._macos_ep_addrs = ep_nums;

                                    send_completion!(txn_id);
                                }
                            }
                        } else {
                            send_error!(txn_id, InvalidNumber);
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
            protocol::RequestMessage::ReleaseInterface { sid, txn_id, value } => {
                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    if !usb_dev.opened {
                        send_error!(txn_id, InvalidState);
                    } else {
                        if let Some(iface_state) = usb_dev.current_if_state.get_mut(&value) {
                            if !iface_state.claimed {
                                send_error!(txn_id, InvalidState);
                            } else {
                                log::debug!(
                                    "device release interface 0x{:02x}, sid = {}, txn = {}",
                                    value,
                                    sid,
                                    txn_id
                                );

                                if let Err(ret) = usb_dev._macos_ifaces
                                    [iface_state._macos_iface_idx]
                                    .USBInterfaceClose()
                                {
                                    log::warn!(
                                        "USBInterfaceClose failed {}, sid = {}, txn = {}, ret = {:08x} ",
                                        iface_state._macos_iface_idx,
                                        sid,
                                        txn_id,
                                        ret
                                    );
                                    send_error!(txn_id, TransferError);
                                } else {
                                    // Release interface successful
                                    iface_state.claimed = false;

                                    // Update this !@#$ list
                                    for ep in iface_state._macos_ep_addrs.drain(..) {
                                        usb_dev._ep_to_idx.remove(&ep);
                                    }

                                    send_completion!(txn_id);
                                }
                            }
                        } else {
                            send_error!(txn_id, InvalidNumber);
                        }
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
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
                if len > u16::MAX as usize {
                    send_error!(txn_id, RequestTooBig);
                    return true;
                }

                // timeout of 0 --> no timeout
                let timeout = _timeout_internal.unwrap_or_default();

                let sid = sid.parse::<u64>().expect("received malformed request");

                let mut devices = self.usb_devices.borrow_mut();
                if let Some(usb_dev) = devices.get_mut(&sid) {
                    log::debug!(
                        "control transfer, sid = {}, txn = {}, {:02x} {:02x} {:04x} {:04x} {:04x} {:02x?}",
                        sid,
                        txn_id,
                        request_type,
                        request,
                        value,
                        index,
                        len as u16,
                        buf
                    );

                    let xfer = USBTransfer {
                        dir,
                        txn_id: txn_id.clone(),
                        buf,
                    };

                    if let Err(ret) = usb_dev._macos_dev.ctrl_xfer(
                        xfer,
                        request_type,
                        request,
                        value,
                        index,
                        len as u16,
                        timeout as u32,
                    ) {
                        // NOTE: A removed device doesn't seem to generate errors here
                        log::warn!(
                            "DeviceRequestAsyncTO failed, sid = {}, txn = {}, ret = {:08x} ",
                            sid,
                            txn_id,
                            ret
                        );
                        send_error!(txn_id, TransferError);
                    }
                } else {
                    send_error!(txn_id, DeviceNotFound);
                }
            }
        }

        true
    }

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
                panic!("kevent poll failed: {:?}", std::io::Error::last_os_error());
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

                if let Some(usb_dev) = USBDevice::setup(item, self_) {
                    // Prepare notification (before we stuff the object away)
                    let notif = protocol::ResponseMessage::NewDevice {
                        sid: sessionid.to_string(),

                        bcdUSB: usb_dev.device_descriptor.bcdUSB,
                        bDeviceClass: usb_dev.device_descriptor.bDeviceClass,
                        bDeviceSubClass: usb_dev.device_descriptor.bDeviceSubClass,
                        bDeviceProtocol: usb_dev.device_descriptor.bDeviceProtocol,
                        idVendor: usb_dev.device_descriptor.idVendor,
                        idProduct: usb_dev.device_descriptor.idProduct,
                        bcdDevice: usb_dev.device_descriptor.bcdDevice,
                        manufacturer: usb_dev.vendor_name.clone(),
                        product: usb_dev.product_name.clone(),
                        serial: usb_dev.serial_number.clone(),

                        current_config: usb_dev.current_configuration_id,
                        configs: usb_dev.reformatted_config_descriptors.clone(),
                    };

                    let mut devices = self_.usb_devices.borrow_mut();
                    let old_dev = devices.insert(sessionid, usb_dev);
                    if old_dev.is_some() {
                        log::warn!("Got a duplicate sessionID?? 0x{:x}", sessionid);
                    }

                    // Send notification
                    let notif = serde_json::to_string(&notif).unwrap();
                    write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
                } else {
                    log::warn!("Device setup failed! session = 0x{:x}", sessionid);
                }
            } else {
                log::warn!("Got plug notification without a sessionID??");
                unsafe { IOObjectRelease(item) };
            }
        }
    }

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

    extern "C" fn iokit_usb_completion(
        refcon: *const (),
        result: libc::kern_return_t,
        arg0: *const (),
    ) {
        // Recover ownership of the transfer
        // SAFETY: This was previously allocated via Box
        let mut xfer = unsafe { Box::from_raw(refcon as *mut USBTransfer) };

        let actual_len = arg0 as usize;
        if xfer.dir == USBTransferDirection::DeviceToHost {
            // Update the size of received data
            unsafe {
                xfer.buf.set_len(actual_len);
            }
        }

        log::debug!(
            "request {} finished, err {:08x}, buf {:02x?}",
            xfer.txn_id,
            result,
            xfer.buf
        );

        // Send notification
        if result == kIOUSBPipeStalled {
            let notif = protocol::ResponseMessage::RequestError {
                txn_id: xfer.txn_id,
                error: protocol::Errors::Stall,
                bytes_written: actual_len as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else if result == 0 || result == kIOReturnOverrun {
            let babble = result == kIOReturnOverrun;
            let data = if xfer.dir == USBTransferDirection::DeviceToHost {
                Some(URL_SAFE_NO_PAD.encode(&xfer.buf))
            } else {
                None
            };
            let notif = protocol::ResponseMessage::RequestComplete {
                txn_id: xfer.txn_id,
                babble,
                data,
                bytes_written: actual_len as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        } else {
            let notif = protocol::ResponseMessage::RequestError {
                txn_id: xfer.txn_id,
                error: protocol::Errors::TransferError,
                bytes_written: actual_len as u64,
            };
            let notif = serde_json::to_string(&notif).unwrap();
            write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
        }

        // n.b. the xfer will now be deallocated automagically
    }
}
impl Drop for USBStubEngine {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self._plug_notifications);
            IOObjectRelease(self._unplug_notifications);
            IONotificationPortDestroy(self._io_notification_port);
            libc::close(self.kqueue);
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
    while state.run_loop() {}

    log::info!("awawausb stub exiting!");
}
