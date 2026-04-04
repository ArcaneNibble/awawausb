//! Data _actually_ defined by Chapter 9 of the USB specification

use core::fmt::Debug;

/// Requests (`bRequest` in a control transfer)
pub mod requests {
    pub const GET_STATUS: u8 = 0;
    pub const CLEAR_FEATURE: u8 = 1;
    pub const SET_FEATURE: u8 = 3;
    pub const SET_ADDRESS: u8 = 5;
    pub const GET_DESCRIPTOR: u8 = 6;
    pub const SET_DESCRIPTOR: u8 = 7;
    pub const GET_CONFIGURATION: u8 = 8;
    pub const SET_CONFIGURATION: u8 = 9;
    pub const GET_INTERFACE: u8 = 10;
    pub const SET_INTERFACE: u8 = 11;
    pub const SYNCH_FRAME: u8 = 12;
}

/// Descriptor types
pub mod descriptor_types {
    pub const DEVICE: u8 = 1;
    pub const CONFIGURATION: u8 = 2;
    pub const STRING: u8 = 3;
    pub const INTERFACE: u8 = 4;
    pub const ENDPOINT: u8 = 5;
    pub const DEVICE_QUALIFIER: u8 = 6;
    pub const OTHER_SPEED_CONFIGURATION: u8 = 7;
    pub const INTERFACE_POWER: u8 = 8;
}

/// Features (for [`SET_FEATURE`](requests::SET_FEATURE))
pub mod features {
    pub const ENDPOINT_HALT: u8 = 0;
    pub const DEVICE_REMOTE_WAKEUP: u8 = 1;
    pub const TEST_MODE: u8 = 2;
}

// Stuff for bmRequestType
pub const REQ_DIR_H2D: u8 = 0x00;
pub const REQ_DIR_D2H: u8 = 0x80;

pub const REQ_TY_STANDARD: u8 = 0 << 5;
pub const REQ_TY_CLASS: u8 = 1 << 5;
pub const REQ_TY_VENDOR: u8 = 2 << 5;
pub const REQ_TY_RESERVED: u8 = 3 << 5;

pub const REQ_RECP_DEVICE: u8 = 0;
pub const REQ_RECP_INTERFACE: u8 = 1;
pub const REQ_RECP_ENDPOINT: u8 = 2;
pub const REQ_RECP_OTHER: u8 = 3;

// Stuff for config desc bmAttributes
pub const CFG_ATTR_SELF_POWER: u8 = 0x40;
pub const CFG_ATTR_REMOTE_WAKEUP: u8 = 0x20;

// Stuff for endpoint address
pub const EP_DIR_OUT: u8 = 0;
pub const EP_DIR_IN: u8 = 0x80;

// Stuff for endpoint desc bmAttributes
pub const EP_TY_CONTROL: u8 = 0;
pub const EP_TY_ISOC: u8 = 1;
pub const EP_TY_BULK: u8 = 2;
pub const EP_TY_INTERRUPT: u8 = 3;

pub const EP_ISOC_SYNC_NONE: u8 = 0 << 2;
pub const EP_ISOC_SYNC_ASYNC: u8 = 1 << 2;
pub const EP_ISOC_SYNC_ADAPTIVE: u8 = 2 << 2;
pub const EP_ISOC_SYNC_SYNC: u8 = 3 << 2;

pub const EP_ISOC_USAGE_DATA: u8 = 0 << 4;
pub const EP_ISOC_USAGE_FB: u8 = 1 << 4;
pub const EP_ISOC_USAGE_IMPLICIT_FB: u8 = 2 << 4;

/// The `bmRequestType` bitfield in a control transfer
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct bmRequestType(pub u8);
impl From<u8> for bmRequestType {
    fn from(value: u8) -> Self {
        Self(value)
    }
}
impl From<bmRequestType> for u8 {
    fn from(value: bmRequestType) -> Self {
        value.0
    }
}
impl core::fmt::Debug for bmRequestType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "0x{:02x} (", self.0)?;

        if self.0 & REQ_DIR_D2H != 0 {
            write!(f, "D->H ")?;
        } else {
            write!(f, "H->D ")?;
        }

        match self.0 >> 5 & 0b11 {
            0 => write!(f, "std ")?,
            1 => write!(f, "cls ")?,
            2 => write!(f, "vnd ")?,
            3 => write!(f, "rsv ")?,
            _ => unreachable!(),
        }

        match self.0 & 0b11111 {
            0 => write!(f, "dev")?,
            1 => write!(f, "if")?,
            2 => write!(f, "ep")?,
            _ => write!(f, "{}", self.0 & 0b11111)?,
        }

        write!(f, ")")?;
        Ok(())
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct GenericDescriptorHeader {
    pub bLength: u8,
    pub bDescriptorType: u8,
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct DeviceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: u16,
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,
    pub idVendor: u16,
    pub idProduct: u16,
    pub bcdDevice: u16,
    pub iManufacturer: u8,
    pub iProduct: u8,
    pub iSerialNumber: u8,
    pub bNumConfigurations: u8,
}
impl DeviceDescriptor {
    pub const fn validate(&self) -> bool {
        if (self.bLength as usize) < core::mem::size_of::<Self>() {
            return false;
        }
        if self.bDescriptorType != descriptor_types::DEVICE {
            return false;
        }
        match self.bMaxPacketSize0 {
            8 | 16 | 32 | 64 => {}
            _ => return false,
        }

        true
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct DeviceQualifierDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: u16,
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,
    pub bNumConfigurations: u8,
    pub bReserved: u8,
}
impl DeviceQualifierDescriptor {
    pub const fn validate(&self) -> bool {
        if (self.bLength as usize) < core::mem::size_of::<Self>() {
            return false;
        }
        if self.bDescriptorType != descriptor_types::DEVICE_QUALIFIER {
            return false;
        }
        match self.bMaxPacketSize0 {
            8 | 16 | 32 | 64 => {}
            _ => return false,
        }

        true
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct ConfigDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wTotalLength: u16,
    pub bNumInterfaces: u8,
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,
    pub bmAttributes: u8,
    pub bMaxPower: u8,
}
impl ConfigDescriptor {
    pub const fn validate(&self) -> bool {
        if (self.bLength as usize) < core::mem::size_of::<Self>() {
            return false;
        }
        if self.bDescriptorType != descriptor_types::CONFIGURATION
            && self.bDescriptorType != descriptor_types::OTHER_SPEED_CONFIGURATION
        {
            return false;
        }

        true
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct InterfaceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bNumEndpoints: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,
}
impl InterfaceDescriptor {
    pub const fn validate(&self) -> bool {
        if (self.bLength as usize) < core::mem::size_of::<Self>() {
            return false;
        }
        if self.bDescriptorType != descriptor_types::INTERFACE {
            return false;
        }

        true
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct EndpointDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: u16,
    pub bInterval: u8,
}
impl EndpointDescriptor {
    pub const fn validate(&self) -> bool {
        if (self.bLength as usize) < core::mem::size_of::<Self>() {
            return false;
        }
        if self.bDescriptorType != descriptor_types::ENDPOINT {
            return false;
        }

        true
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct StringDescriptorFixed<const LEN: usize> {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bytes: [u8; LEN],
}
impl<const LEN: usize> StringDescriptorFixed<LEN> {
    pub const fn unsize(&self) -> &StringDescriptor {
        let ptr = self as *const StringDescriptorFixed<LEN>;
        unsafe {
            // SAFETY: Uses undocumented layout of rustc fat pointers
            #[repr(C)]
            struct FatPointer {
                ptr: *const (),
                size: usize,
            }
            let fatptr = FatPointer {
                ptr: ptr as *const (),
                size: LEN,
            };
            let fatptr: *const StringDescriptor = core::mem::transmute(fatptr);
            &*fatptr
        }
    }
}
#[macro_export]
macro_rules! make_string_desc {
    ($id:ident = $s:expr) => {
        const $id: $crate::ch9_core::StringDescriptorFixed<{ $s.len() * 2 }> =
            $crate::ch9_core::StringDescriptorFixed {
                bLength: (2 + $s.len() * 2) as u8,
                bDescriptorType: descriptor_types::STRING,
                // SAFETY: Only bit-twiddling u16s into u8s
                bytes: unsafe { core::mem::transmute(utf16_lit::utf16!($s)) },
            };
    };
}

#[repr(C, packed)]
#[allow(non_snake_case)]
#[derive(Eq)]
pub struct StringDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bytes: [u8],
}
impl PartialEq for StringDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.bLength == other.bLength
            && self.bDescriptorType == other.bDescriptorType
            && self.bytes == other.bytes
    }
}
impl Debug for StringDescriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StringDescriptor")
            .field("bLength", &self.bLength)
            .field("bDescriptorType", &self.bDescriptorType)
            .field("bytes", &&self.bytes)
            .finish()
    }
}
impl core::hash::Hash for StringDescriptor {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.bLength.hash(state);
        self.bDescriptorType.hash(state);
        self.bytes.hash(state);
    }
}
/// Coerce the payload data into a u16 slice pointer
///
/// Only a pointer is possible, because alignment
impl From<&StringDescriptor> for *const [u16] {
    fn from(value: &StringDescriptor) -> Self {
        let bytes = value.bytes.as_ptr();
        let u16_len = value.bytes.len() / 2;
        core::ptr::slice_from_raw_parts(bytes as *const u16, u16_len)
    }
}
struct U16PtrIterator(*const [u16]);
impl Iterator for U16PtrIterator {
    type Item = u16;
    fn next(&mut self) -> Option<Self::Item> {
        if self.0.len() == 0 {
            None
        } else {
            let item_ptr = self.0 as *const u16;
            // SAFETY: We know we have a valid slice here
            // (private struct, must construct it correctly)
            let item = unsafe { item_ptr.read_unaligned() };
            let item_ptr = unsafe { item_ptr.add(1) };
            let new_sz = self.0.len() - 1;
            self.0 = core::ptr::slice_from_raw_parts(item_ptr, new_sz);
            Some(item)
        }
    }
}
impl StringDescriptor {
    /// Decode the string descriptor as UTF-16
    pub fn payload(&self) -> core::char::DecodeUtf16<impl Iterator<Item = u16>> {
        let u16iter = U16PtrIterator(self.into());
        char::decode_utf16(u16iter)
    }

    pub fn from_bytes<'a>(bytes: &'a [u8]) -> (&'a Self, &'a [u8]) {
        assert!(bytes.len() >= core::mem::size_of::<GenericDescriptorHeader>());

        // Check desc type, len
        let bytes_ptr = bytes.as_ptr();
        let desc_hdr = bytes_ptr as *const GenericDescriptorHeader;
        let desc_sz = unsafe {
            // SAFETY: Input is a valid slice, of sufficient length, and we know alignment = 1
            assert_eq!((*desc_hdr).bDescriptorType, descriptor_types::STRING);
            (*desc_hdr).bLength as usize
        };
        let bytes_left = bytes.split_at(desc_sz).1;

        // Mangle Rust nonsense
        let str_desc = unsafe {
            // SAFETY: Uses undocumented layout of rustc fat pointers
            #[repr(C)]
            struct FatPointer {
                ptr: *const u8,
                size: usize,
            }
            let fatptr = FatPointer {
                ptr: bytes_ptr,
                size: desc_sz - 2,
            };
            let fatptr: *const StringDescriptor = core::mem::transmute(fatptr);
            &*fatptr
        };

        (str_desc, bytes_left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn test_req_debug_formatting() {
        extern crate alloc;

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0x80);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0x80 (D->H std dev)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0x00);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0x00 (H->D std dev)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0xA0);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0xa0 (D->H cls dev)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0xC0);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0xc0 (D->H vnd dev)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0x81);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0x81 (D->H std if)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0x82);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0x82 (D->H std ep)");

        let mut s = alloc::string::String::new();
        let req = bmRequestType(0x83);
        write!(&mut s, "{:?}", req).unwrap();
        assert_eq!(s, "0x83 (D->H std 3)");
    }

    #[test]
    fn test_sizes() {
        assert_eq!(core::mem::size_of::<DeviceDescriptor>(), 18);
        assert_eq!(core::mem::size_of::<DeviceQualifierDescriptor>(), 10);
        assert_eq!(core::mem::size_of::<ConfigDescriptor>(), 9);
        assert_eq!(core::mem::size_of::<InterfaceDescriptor>(), 9);
        assert_eq!(core::mem::size_of::<EndpointDescriptor>(), 7);

        assert_eq!(core::mem::align_of::<DeviceDescriptor>(), 1);
        assert_eq!(core::mem::align_of::<DeviceQualifierDescriptor>(), 1);
        assert_eq!(core::mem::align_of::<ConfigDescriptor>(), 1);
        assert_eq!(core::mem::align_of::<InterfaceDescriptor>(), 1);
        assert_eq!(core::mem::align_of::<EndpointDescriptor>(), 1);
    }

    #[test]
    fn test_string_desc() {
        extern crate std;

        make_string_desc!(TEST = "asdf");

        assert_eq!(TEST.bLength, 10);
        assert_eq!(TEST.bytes, [b'a', 0, b's', 0, b'd', 0, b'f', 0]);

        let test_ptr = TEST.unsize();
        assert_eq!(core::mem::size_of_val(test_ptr), 10);
        assert_eq!(core::mem::align_of_val(test_ptr), 1);

        let parsed = test_ptr
            .payload()
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<std::string::String>();
        assert_eq!(parsed, "asdf");

        let test_as_bytes = unsafe {
            core::slice::from_raw_parts(
                &TEST as *const StringDescriptorFixed<_> as *const u8,
                core::mem::size_of_val(&TEST),
            )
        };
        let (test_ptr_2, test_left) = StringDescriptor::from_bytes(test_as_bytes);
        assert_eq!(test_left.len(), 0);
        let parsed = test_ptr_2
            .payload()
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<std::string::String>();
        assert_eq!(parsed, "asdf");
    }
}
