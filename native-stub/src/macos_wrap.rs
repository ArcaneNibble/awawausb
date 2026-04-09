use std::cell::{Cell, RefCell};
use std::ptr;
use std::rc::Rc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
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
        txn_id: &str,
        mut buf: Vec<u8>,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: u32,
        dir: crate::USBTransferDirection,
    ) -> Result<(), kern_return_t> {
        assert!(length as usize <= buf.capacity());

        // Prepare our object, which is a Box on the heap so that it doesn't move
        let buf_ptr = buf.as_mut_ptr();
        let xfer = USBTransfer {
            dir,
            txn_id: txn_id.to_owned(),
            buf,
            _macos_iface: None,
        };
        let xfer_ptr = Box::into_raw(Box::new(xfer));

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
                iokit_usb_completion,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipeProperties {
    direction: u8,
    number: u8,
    transfer_type: u8,
    max_packet_size: u16,
    interval: u8,
}

/// A USB transfer which is currently in-flight
///
/// This would usually be called an "URB" (USB Request Block).
///
/// When the transfer has been submitted, this struct is "owned by"
/// the operating system. We reclaim ownership when we get a completion.
#[derive(Debug)]
pub struct USBTransfer {
    pub dir: crate::USBTransferDirection,
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

    #[cfg(target_os = "macos")]
    _macos_iface: Option<Rc<RefCell<IOUSBInterfaceStruct>>>,
}

/// A USB transfer's metadata, specifically for isochronous requests
#[derive(Debug)]
pub struct USBTransferIsoc {
    pub dir: crate::USBTransferDirection,
    pub txn_id: String,
    pub buf: Vec<u8>,
    pub num_packets: usize,

    _macos_frames: Box<[IOUSBIsocFrame]>,
    _macos_iface: Rc<RefCell<IOUSBInterfaceStruct>>,
}

#[derive(Debug)]
pub struct IOUSBInterfaceStruct(
    *mut *const IOUSBInterfaceStruct197,
    pub(crate) *const Cell<usize>,
);
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

            Self(device as *mut *const IOUSBInterfaceStruct197, ptr::null())
        }
    }

    pub fn CreateInterfaceAsyncPort(&mut self) -> Result<mach_port_t, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).CreateInterfaceAsyncPort)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
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
    pub fn GetNumEndpoints(&mut self) -> Result<u8, kern_return_t> {
        let mut out = 0;
        let ret = unsafe { ((**self.0).GetNumEndpoints)(self.0 as *const (), &mut out) };
        if ret != 0 { Err(ret) } else { Ok(out) }
    }
    pub fn SetAlternateInterface(&mut self, alt: u8) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).SetAlternateInterface)(self.0 as *const (), alt) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
    pub fn GetPipeProperties(&mut self, pipe_ref: u8) -> Result<PipeProperties, kern_return_t> {
        let mut direction = 0;
        let mut number = 0;
        let mut transfer_type = 0;
        let mut max_packet_size = 0;
        let mut interval = 0;
        let ret = unsafe {
            ((**self.0).GetPipeProperties)(
                self.0 as *const (),
                pipe_ref,
                &mut direction,
                &mut number,
                &mut transfer_type,
                &mut max_packet_size,
                &mut interval,
            )
        };
        if ret != 0 {
            Err(ret)
        } else {
            Ok(PipeProperties {
                direction,
                number,
                transfer_type,
                max_packet_size,
                interval,
            })
        }
    }

    pub fn get_ep_addrs(&mut self) -> Vec<u8> {
        if let Ok(num_eps) = self.GetNumEndpoints() {
            let mut ep_addrs = Vec::with_capacity(num_eps as usize);
            for i in 0..num_eps {
                if let Ok(pipe_props) = self.GetPipeProperties(i + 1) {
                    ep_addrs
                        .push(pipe_props.number | if pipe_props.direction != 0 { 0x80 } else { 0 });
                } else {
                    log::warn!("Could not get pipe {} info!", i + 1);
                }
            }
            ep_addrs
        } else {
            log::warn!("Could not get endpoint list!");
            Vec::new()
        }
    }

    pub fn data_xfer(
        &mut self,
        self2: Rc<RefCell<Self>>,
        txn_id: &str,
        mut buf: Vec<u8>,
        pipe_ref: u8,
        length: u32,
        dir: crate::USBTransferDirection,
    ) -> Result<(), kern_return_t> {
        assert!(length as usize <= buf.capacity());
        assert_eq!(self as *mut _, self2.as_ptr());

        // Prepare our object, which is a Box on the heap so that it doesn't move
        let buf_ptr = buf.as_mut_ptr();
        let xfer = USBTransfer {
            dir,
            txn_id: txn_id.to_owned(),
            buf,
            _macos_iface: Some(self2),
        };
        let xfer_ptr = Box::into_raw(Box::new(xfer));

        let ret = match dir {
            crate::USBTransferDirection::HostToDevice => unsafe {
                ((**self.0).WritePipeAsync)(
                    self.0 as *const (),
                    pipe_ref,
                    buf_ptr as *const (),
                    length,
                    iokit_usb_completion,
                    xfer_ptr as *const (),
                )
            },
            crate::USBTransferDirection::DeviceToHost => unsafe {
                ((**self.0).ReadPipeAsync)(
                    self.0 as *const (),
                    pipe_ref,
                    buf_ptr as *mut (),
                    length,
                    iokit_usb_completion,
                    xfer_ptr as *const (),
                )
            },
        };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }

    pub fn isoc_xfer(
        &mut self,
        self2: Rc<RefCell<Self>>,
        txn_id: &str,
        mut buf: Vec<u8>,
        pipe_ref: u8,
        pkt_len: Vec<u32>,
        total_len: usize,
        dir: crate::USBTransferDirection,
    ) -> Result<(), kern_return_t> {
        assert!(total_len <= buf.capacity());
        assert_eq!(self as *mut _, self2.as_ptr());

        // IOKit/kernel frame list
        let macos_isoc_frames = pkt_len
            .iter()
            .map(|x| IOUSBIsocFrame {
                frStatus: 0,
                frReqCount: *x as u16,
                frActCount: 0,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        // "Back up" value we need later
        let buf_ptr = buf.as_mut_ptr();

        // Callback state
        let mut xfer = Box::new(USBTransferIsoc {
            dir,
            txn_id: txn_id.to_owned(),
            buf,
            num_packets: pkt_len.len(),
            _macos_frames: macos_isoc_frames,
            _macos_iface: self2,
        });

        // We need to make sure this callback state's ownership is transferred away
        let frame_list = xfer._macos_frames.as_mut_ptr();
        let xfer_ptr = Box::into_raw(xfer);

        let mut bus_frame = 0;
        let mut bus_time = 0;
        let ret = unsafe {
            ((**self.0).GetBusFrameNumber)(self.0 as *const (), &mut bus_frame, &mut bus_time)
        };
        if ret != 0 {
            return Err(ret);
        }

        // "Some time later"
        bus_frame += 4;

        let ret = match dir {
            crate::USBTransferDirection::HostToDevice => unsafe {
                ((**self.0).WriteIsochPipeAsync)(
                    self.0 as *const (),
                    pipe_ref,
                    buf_ptr as *const (),
                    bus_frame,
                    pkt_len.len() as u32,
                    frame_list,
                    iokit_usb_completion_isoc,
                    xfer_ptr as *const (),
                )
            },
            crate::USBTransferDirection::DeviceToHost => unsafe {
                ((**self.0).ReadIsochPipeAsync)(
                    self.0 as *const (),
                    pipe_ref,
                    buf_ptr as *mut (),
                    bus_frame,
                    pkt_len.len() as u32,
                    frame_list,
                    iokit_usb_completion_isoc,
                    xfer_ptr as *const (),
                )
            },
        };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }

    pub fn ClearPipeStallBothEnds(&mut self, pipe_ref: u8) -> Result<(), kern_return_t> {
        let ret = unsafe { ((**self.0).ClearPipeStallBothEnds)(self.0 as *const (), pipe_ref) };
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
}
impl Drop for IOUSBInterfaceStruct {
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

extern "C" fn iokit_usb_completion(
    refcon: *const (),
    result: libc::kern_return_t,
    arg0: *const (),
) {
    // Recover ownership of the transfer
    // SAFETY: This was previously allocated via Box
    let mut xfer = unsafe { Box::from_raw(refcon as *mut USBTransfer) };

    let actual_len = arg0 as usize;
    if xfer.dir == crate::USBTransferDirection::DeviceToHost {
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
        let notif = crate::protocol::ResponseMessage::RequestError {
            txn_id: xfer.txn_id,
            error: crate::protocol::Errors::Stall,
            bytes_written: actual_len as u64,
        };
        let notif = serde_json::to_string(&notif).unwrap();
        crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    } else if result == 0 || result == kIOReturnOverrun {
        let babble = result == kIOReturnOverrun;
        let data = if xfer.dir == crate::USBTransferDirection::DeviceToHost {
            Some(URL_SAFE_NO_PAD.encode(&xfer.buf))
        } else {
            None
        };
        let notif = crate::protocol::ResponseMessage::RequestComplete {
            txn_id: xfer.txn_id,
            babble,
            data,
            bytes_written: actual_len as u64,
        };
        let notif = serde_json::to_string(&notif).unwrap();
        crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    } else {
        let notif = crate::protocol::ResponseMessage::RequestError {
            txn_id: xfer.txn_id,
            error: crate::protocol::Errors::TransferError,
            bytes_written: actual_len as u64,
        };
        let notif = serde_json::to_string(&notif).unwrap();
        crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    }

    // n.b. the xfer will now be deallocated automagically
}

extern "C" fn iokit_usb_completion_isoc(
    refcon: *const (),
    result: libc::kern_return_t,
    _arg0: *const (),
) {
    // Recover ownership of the transfer
    // SAFETY: This was previously allocated via Box
    let mut xfer = unsafe { Box::from_raw(refcon as *mut USBTransferIsoc) };

    let mut had_unwanted_error = false;

    let mut pkt_status = Vec::with_capacity(xfer.num_packets);
    let mut pkt_lens = Vec::with_capacity(xfer.num_packets);
    let mut total_len = 0;
    for i in 0..xfer.num_packets {
        pkt_status.push(match xfer._macos_frames[i].frStatus {
            0 => crate::protocol::IsocPacketState::Ok,
            #[allow(non_upper_case_globals)]
            kIOReturnOverrun => crate::protocol::IsocPacketState::Babble,
            _ => {
                had_unwanted_error = true;
                crate::protocol::IsocPacketState::Error
            }
        });
        pkt_lens.push(xfer._macos_frames[i].frActCount as u32);
        total_len += xfer._macos_frames[i].frActCount as usize;
    }
    let data = if xfer.dir == crate::USBTransferDirection::DeviceToHost {
        // Update the size of received data
        unsafe {
            xfer.buf.set_len(total_len);
        }
        Some(URL_SAFE_NO_PAD.encode(&xfer.buf))
    } else {
        None
    };

    log::debug!(
        "isoc request {} finished, err {:08x}, buf {:02x?} status {:08x?} len {:?}",
        xfer.txn_id,
        result,
        xfer.buf,
        pkt_status,
        pkt_lens,
    );

    if had_unwanted_error || (result != 0 && result != kIOReturnOverrun) {
        // An error, of a type we don't "tolerate"
        let notif = crate::protocol::ResponseMessage::RequestError {
            txn_id: xfer.txn_id,
            error: crate::protocol::Errors::TransferError,
            bytes_written: total_len as u64,
        };
        let notif = serde_json::to_string(&notif).unwrap();
        crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    } else {
        // Success
        let notif = crate::protocol::ResponseMessage::IsocRequestComplete {
            txn_id: xfer.txn_id,
            data,
            pkt_status,
            pkt_len: pkt_lens,
        };
        let notif = serde_json::to_string(&notif).unwrap();
        crate::stdio_unix::write_stdout_msg(notif.as_bytes()).expect("failed to write stdout");
    }
}
