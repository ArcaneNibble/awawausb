#![no_std]

pub mod ch9_core;
pub mod interface_association_descriptor;

pub trait USBDescriptor {
    fn from_bytes(b: &[u8]) -> Option<(&Self, &[u8])>
    where
        Self: Sized,
    {
        assert_eq!(core::mem::align_of::<Self>(), 1);
        let expected_sz = core::mem::size_of::<Self>();
        if b.len() < expected_sz {
            None
        } else {
            let generic_desc = ch9_core::GenericDescriptorHeader::from_bytes(b).unwrap().0;
            let stated_len = generic_desc.bLength as usize;
            let rest = b.split_at(stated_len).1;

            Some((unsafe { &*(b.as_ptr() as *const Self) }, rest))
        }
    }

    fn to_bytes(&self) -> &[u8]
    where
        Self: Sized,
    {
        let sz = core::mem::size_of::<Self>();
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, sz) }
    }
}

/// Things which can be decoded from a configuration descriptor
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum DescriptorRef<'a> {
    Config(&'a ch9_core::ConfigDescriptor),
    String(&'a ch9_core::StringDescriptor),
    Interface(&'a ch9_core::InterfaceDescriptor),
    Endpoint(&'a ch9_core::EndpointDescriptor),
    UnknownDescriptor(&'a [u8]),
}

pub fn parse_descriptor_set(inp: &[u8]) -> ParseDescriptorSet<'_> {
    ParseDescriptorSet(inp)
}

/// Decode things which can exist in a configuration descriptor
pub struct ParseDescriptorSet<'a>(&'a [u8]);
impl<'a> Iterator for ParseDescriptorSet<'a> {
    type Item = DescriptorRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((generic_desc, _)) = ch9_core::GenericDescriptorHeader::from_bytes(self.0) {
            let desc_ty = generic_desc.bDescriptorType;

            let (ref_, rest) = match desc_ty {
                ch9_core::descriptor_types::CONFIGURATION
                | ch9_core::descriptor_types::OTHER_SPEED_CONFIGURATION => {
                    let (desc, rest) = ch9_core::ConfigDescriptor::from_bytes(self.0)?;
                    (DescriptorRef::Config(desc), rest)
                }
                ch9_core::descriptor_types::INTERFACE => {
                    let (desc, rest) = ch9_core::InterfaceDescriptor::from_bytes(self.0)?;
                    (DescriptorRef::Interface(desc), rest)
                }
                ch9_core::descriptor_types::STRING => {
                    let (desc, rest) = ch9_core::StringDescriptor::from_bytes(self.0)?;
                    (DescriptorRef::String(desc), rest)
                }
                ch9_core::descriptor_types::ENDPOINT => {
                    let (desc, rest) = ch9_core::EndpointDescriptor::from_bytes(self.0)?;
                    (DescriptorRef::Endpoint(desc), rest)
                }
                _ => {
                    let len = generic_desc.bLength as usize;
                    (
                        DescriptorRef::UnknownDescriptor(unsafe {
                            core::slice::from_raw_parts(self.0.as_ptr(), len)
                        }),
                        &self.0[len..],
                    )
                }
            };

            self.0 = rest;
            Some(ref_)
        } else {
            None
        }
    }
}
