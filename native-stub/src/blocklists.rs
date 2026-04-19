pub fn is_blocked_device(vid: u16, pid: u16) -> bool {
    // XXX this doesn't support bcdDevice filtering, because there are no such examples
    match (vid, pid) {
        (0x096e, 0x0850) |  // KEY-ID
        (0x096e, 0x0852) |  // Feitian
        (0x096e, 0x0853) |  // Feitian
        (0x096e, 0x0854) |  // Feitian
        (0x096e, 0x0856) |  // Feitian
        (0x096e, 0x0858) |  // Feitian USB+NFC
        (0x096e, 0x085a) |  // Feitian
        (0x096e, 0x085b) |  // Feitian
        (0x096e, 0x0880) |  // HyperFIDO

        (0x09c3, 0x0023) |  // HID Global BlueTrust Token

        // # Yubikey devices. https://crbug.com/818807
        (0x1050, 0x0010) |
        (0x1050, 0x0018) |
        (0x1050, 0x0030) |
        (0x1050, 0x0110) |
        (0x1050, 0x0111) |
        (0x1050, 0x0112) |
        (0x1050, 0x0113) |
        (0x1050, 0x0114) |
        (0x1050, 0x0115) |
        (0x1050, 0x0116) |
        (0x1050, 0x0120) |
        (0x1050, 0x0200) |
        (0x1050, 0x0211) |
        (0x1050, 0x0401) |
        (0x1050, 0x0402) |
        (0x1050, 0x0403) |
        (0x1050, 0x0404) |
        (0x1050, 0x0405) |
        (0x1050, 0x0406) |
        (0x1050, 0x0407) |
        (0x1050, 0x0410) |

        (0x10c4, 0x8acf) |  // U2F Zero
        (0x18d1, 0x5026) |  // Titan
        (0x1a44, 0x00bb) |  // VASCO
        (0x1d50, 0x60fc) |  // OnlyKey
        (0x1e0d, 0xf1ae) |  // Keydo AES
        (0x1e0d, 0xf1d0) |  // Neowave Keydo
        (0x1ea8, 0xf025) |  // Thetis
        (0x20a0, 0x4287) |  // Nitrokey
        (0x24dc, 0x0101) |  // JaCarta
        (0x2581, 0xf1d0) |  // Happlink
        (0x2abe, 0x1002) |  // Bluink
        (0x2ccf, 0x0880)    // Feitian USB:HyperFIDO
        => true,
        _ => false,
    }
}
