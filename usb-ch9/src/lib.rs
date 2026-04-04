#![no_std]

pub mod ch9_core;
pub mod interface_association_descriptor;

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
        if self.0.len() < core::mem::size_of::<ch9_core::GenericDescriptorHeader>() {
            None
        } else {
            // Check desc type, len
            let bytes_ptr = self.0.as_ptr();
            let desc_hdr = bytes_ptr as *const ch9_core::GenericDescriptorHeader;
            // SAFETY: Input is a valid slice, of sufficient length, and we know alignment = 1
            let desc_ty = unsafe { (*desc_hdr).bDescriptorType };
            let desc_sz = unsafe { (*desc_hdr).bLength as usize };
            self.0 = self.0.split_at(desc_sz).1;

            Some(match desc_ty {
                ch9_core::descriptor_types::CONFIGURATION
                | ch9_core::descriptor_types::OTHER_SPEED_CONFIGURATION => unsafe {
                    DescriptorRef::Config(&*(bytes_ptr as *const ch9_core::ConfigDescriptor))
                },
                ch9_core::descriptor_types::STRING => unsafe {
                    DescriptorRef::String(
                        ch9_core::StringDescriptor::from_bytes(core::slice::from_raw_parts(
                            bytes_ptr, desc_sz,
                        ))
                        .0,
                    )
                },
                ch9_core::descriptor_types::INTERFACE => unsafe {
                    DescriptorRef::Interface(&*(bytes_ptr as *const ch9_core::InterfaceDescriptor))
                },
                ch9_core::descriptor_types::ENDPOINT => unsafe {
                    DescriptorRef::Endpoint(&*(bytes_ptr as *const ch9_core::EndpointDescriptor))
                },
                _ => DescriptorRef::UnknownDescriptor(unsafe {
                    core::slice::from_raw_parts(bytes_ptr, desc_sz)
                }),
            })
        }
    }
}
