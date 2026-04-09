//! Interface Association Descriptors (IAD) Engineering Change Notice (ECN)

pub const DESC_TYPE_IAD: u8 = 11;

#[repr(C, packed)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(non_snake_case)]
pub struct InterfaceAssociationDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bFirstInterface: u8,
    pub bInterfaceCount: u8,
    pub bFunctionClass: u8,
    pub bFunctionSubClass: u8,
    pub bFunctionProtocol: u8,
    pub iFunction: u8,
}
impl super::USBDescriptor for InterfaceAssociationDescriptor {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sizes() {
        assert_eq!(core::mem::size_of::<InterfaceAssociationDescriptor>(), 8);
        assert_eq!(core::mem::align_of::<InterfaceAssociationDescriptor>(), 1);
    }
}
