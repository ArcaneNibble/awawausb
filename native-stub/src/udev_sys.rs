use std::alloc::Layout;
use std::fmt::Debug;
use std::io;
use std::mem::{self, MaybeUninit};
use std::ptr;

const MONITOR_GROUP_UDEV: u32 = 2;

pub const fn murmur_hash_2(mut key: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1e995;
    const R: u32 = 24;

    let mut h = seed ^ key.len() as u32;

    while key.len() >= 4 {
        // FIXME: This is endianness-dependent??
        let mut k = u32::from_ne_bytes([key[0], key[1], key[2], key[3]]);

        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);

        h = h.wrapping_mul(M);
        h ^= k;

        key = key.split_at(4).1;
    }

    if key.len() >= 3 {
        h ^= (key[2] as u32) << 16;
    }
    if key.len() >= 2 {
        h ^= (key[1] as u32) << 8;
    }
    if key.len() >= 1 {
        h ^= (key[0] as u32) << 0;
    }
    h = h.wrapping_mul(M);

    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;

    h
}

#[repr(C)]
#[derive(Debug)]
pub struct UdevFeedcafeMessageHeader {
    pub libudev_magic: [u8; 8],
    pub version_magic: u32,
    pub header_sz: u32,

    pub properties_off: u32,
    pub properties_len: u32,

    pub subsystem_hash: u32,
    pub devtype_hash: u32,
    pub tag_bloom_hi: u32,
    pub tag_bloom_lo: u32,
}

#[repr(C)]
pub struct UdevProperty<'a> {
    pub key: &'a [u8],
    pub val: Option<&'a [u8]>,
}
impl<'a> Debug for UdevProperty<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdevProperty")
            .field("key", &String::from_utf8_lossy(&self.key))
            .field("val", &self.val.map(|x| String::from_utf8_lossy(x)))
            .finish()
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct UdevFeedCafeMessage {
    pub hdr: UdevFeedcafeMessageHeader,
    pub payload: [u8],
}
impl UdevFeedCafeMessage {
    pub fn mangle_and_validate(&mut self) -> Result<(), ()> {
        if &self.hdr.libudev_magic != b"libudev\x00" {
            return Err(());
        }

        // Mangle endianness
        // NOTE: Cross-endianness usage (qemu-user?) is not properly implemented
        self.hdr.version_magic = u32::from_be(self.hdr.version_magic);
        self.hdr.subsystem_hash = u32::from_be(self.hdr.subsystem_hash);
        self.hdr.devtype_hash = u32::from_be(self.hdr.devtype_hash);
        self.hdr.tag_bloom_hi = u32::from_be(self.hdr.tag_bloom_hi);
        self.hdr.tag_bloom_lo = u32::from_be(self.hdr.tag_bloom_lo);

        if self.hdr.version_magic != 0xfeedcafe {
            return Err(());
        }

        // Validate everything is in range
        if (self.hdr.header_sz as usize) < mem::size_of::<UdevFeedcafeMessageHeader>()
            || (self.hdr.properties_off as usize) < mem::size_of::<UdevFeedcafeMessageHeader>()
        {
            return Err(());
        }
        // Offset within what we have *declared as* `payload`
        let actual_payload_off =
            (self.hdr.properties_off as usize) - mem::size_of::<UdevFeedcafeMessageHeader>();
        let actual_payload_len = self.payload.len() - actual_payload_off;
        if (self.hdr.properties_len as usize) > actual_payload_len {
            return Err(());
        }

        // Finally, all good
        Ok(())
    }

    pub fn properties(&self) -> impl Iterator<Item = UdevProperty> {
        // Offset within what we have *declared as* `payload`
        let actual_payload_off =
            (self.hdr.properties_off as usize) - mem::size_of::<UdevFeedcafeMessageHeader>();
        let mut properties = &self.payload[actual_payload_off..self.hdr.properties_len as usize];

        // Strip trailing null, if there is one
        if let Some((x, rest)) = properties.split_last()
            && *x == 0
        {
            properties = rest;
        }

        properties.split(|x| *x == 0).map(|x| {
            if let Some(eq_idx) = x.iter().position(|x| *x == b'=') {
                UdevProperty {
                    key: &x[..eq_idx],
                    val: Some(&x[eq_idx + 1..]),
                }
            } else {
                UdevProperty { key: x, val: None }
            }
        })
    }
}

#[derive(Debug)]
pub struct UdevNetlinkSocket {
    fd: i32,
}
impl UdevNetlinkSocket {
    pub fn new() -> io::Result<Self> {
        let sock_fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                libc::NETLINK_KOBJECT_UEVENT,
            )
        };
        if sock_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        log::debug!("udev socket is {}", sock_fd);
        let out = Self { fd: sock_fd };

        let mut sa_nl = unsafe { MaybeUninit::<libc::sockaddr_nl>::zeroed().assume_init() };
        sa_nl.nl_family = libc::AF_NETLINK as u16;
        sa_nl.nl_groups = MONITOR_GROUP_UDEV;
        let ret = unsafe {
            libc::bind(
                sock_fd,
                &sa_nl as *const _ as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_nl>() as u32,
            )
        };
        if ret < 0 {
            log::warn!("bind() failed on udev socket!");
            return Err(io::Error::last_os_error());
        }

        Ok(out)
    }

    pub fn get_event(&self) -> io::Result<()> {
        let pkt_sz = unsafe {
            libc::recv(
                self.fd,
                ptr::null_mut(),
                0,
                libc::MSG_PEEK | libc::MSG_TRUNC,
            )
        };
        if pkt_sz < 0 {
            log::warn!("recv(peeking) failed on udev socket!");
            return Err(io::Error::last_os_error());
        }
        let pkt_sz = pkt_sz as usize;

        // let mut buf = Vec::<u8>::with_capacity(pkt_sz as usize);
        let buf_layout =
            Layout::from_size_align(pkt_sz, mem::align_of::<UdevFeedcafeMessageHeader>()).unwrap();
        dbg!(buf_layout);
        let buf = unsafe { std::alloc::alloc(buf_layout) };

        // We use recvmsg, even though we (currently) don't receive creds,
        // in order to make sure we didn't get a truncated message
        // (is that even possible on this socket?)
        let mut iov = libc::iovec {
            iov_base: buf as *mut libc::c_void,
            iov_len: pkt_sz,
        };
        let mut msg = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        let sz2 = unsafe { libc::recvmsg(self.fd, &mut msg, 0) };
        if sz2 < 0 || sz2 as usize != pkt_sz {
            log::warn!("recv() failed on udev socket!");
            return Err(io::Error::last_os_error());
        }
        if pkt_sz < mem::size_of::<UdevFeedcafeMessageHeader>() {
            log::warn!("recv packet way too small on udev socket!");
            return Err(io::Error::from(io::ErrorKind::Other));
        }

        // XXX I don't understand why systemd needs to check the sender UID here.
        // That seems unnecessary, since
        // > Only processes with an effective UID of 0 or the CAP_NET_ADMIN capability
        // > may send or listen to a netlink multicast group.
        // Note^2: The latter doesn't apply here, since
        // > Some Linux kernel subsystems may additionally allow other users
        // > to send and/or receive messages. As at Linux 3.0, the NETLINK_KOBJECT_UEVENT, [...]
        // > groups allow other users to receive messages.
        //
        // > **No groups allow other users to send messages.**

        // Deal with received sizes
        let payload_sz = pkt_sz - mem::size_of::<UdevFeedcafeMessageHeader>();
        struct FatPointer {
            ptr: *mut u8,
            sz: usize,
        }
        let mut pkt: Box<UdevFeedCafeMessage> = unsafe {
            mem::transmute(FatPointer {
                ptr: buf,
                sz: payload_sz,
            })
        };
        pkt.mangle_and_validate().map_err(|_| {
            log::warn!("got a malformed udev packet!");
            io::Error::from(io::ErrorKind::Other)
        })?;
        println!("{:x?}", pkt.hdr);
        let props = pkt.properties().collect::<Vec<_>>();
        println!("{:?}", props);
        println!(
            "{:08x} {:08x}",
            pkt.hdr.subsystem_hash,
            murmur_hash_2(b"usb", 0)
        );
        println!(
            "{:08x} {:08x}",
            pkt.hdr.devtype_hash,
            murmur_hash_2(b"usb_device", 0)
        );

        // todo
        Ok(())
    }
}
impl Drop for UdevNetlinkSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}
