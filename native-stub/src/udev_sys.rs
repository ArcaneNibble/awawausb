use std::alloc::Layout;
use std::collections::HashMap;
use std::fmt::Debug;
use std::io;
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::u32;

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

    pub fn properties(&self) -> impl Iterator<Item = UdevProperty<'_>> {
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
    pub fd: i32,
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

        // Attach (hardcoded for usb) BPF optimization program
        let mut bpf_prog = Vec::new();
        {
            use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_K, BPF_LD, BPF_RET, BPF_W, sock_filter};

            macro_rules! check_u32 {
                ($off:expr, $val:expr) => {
                    bpf_prog.push(sock_filter {
                        code: (BPF_LD | BPF_W | BPF_ABS) as u16,
                        jt: 0,
                        jf: 0,
                        k: $off as u32,
                    });
                    bpf_prog.push(sock_filter {
                        code: (BPF_JMP | BPF_JEQ | BPF_K) as u16,
                        jt: 1, // skip next opcode if equal
                        jf: 0,
                        k: $val,
                    });
                    bpf_prog.push(sock_filter {
                        code: (BPF_RET | BPF_K) as u16,
                        jt: 0,
                        jf: 0,
                        k: 0, // return 0 => no match
                    });
                };
            }

            // Yes, all the endianness here is correct
            check_u32!(
                mem::offset_of!(UdevFeedcafeMessageHeader, libudev_magic),
                u32::from_be_bytes(*b"libu")
            );
            check_u32!(
                mem::offset_of!(UdevFeedcafeMessageHeader, libudev_magic) + 4,
                u32::from_be_bytes(*b"dev\x00")
            );
            check_u32!(
                mem::offset_of!(UdevFeedcafeMessageHeader, version_magic),
                0xfeedcafe
            );
            check_u32!(
                mem::offset_of!(UdevFeedcafeMessageHeader, subsystem_hash),
                murmur_hash_2(b"usb", 0)
            );
            check_u32!(
                mem::offset_of!(UdevFeedcafeMessageHeader, devtype_hash),
                murmur_hash_2(b"usb_device", 0)
            );

            // Finally accept the packet
            bpf_prog.push(sock_filter {
                code: (BPF_RET | BPF_K) as u16,
                jt: 0,
                jf: 0,
                k: u32::MAX,
            });
        }
        let filter = libc::sock_fprog {
            len: bpf_prog.len() as u16,
            filter: bpf_prog.as_mut_ptr(),
        };
        let ret = unsafe {
            libc::setsockopt(
                sock_fd,
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                &filter as *const _ as *const libc::c_void,
                mem::size_of::<libc::sock_fprog>() as u32,
            )
        };
        if ret < 0 {
            log::warn!("attaching bpf failed!");
            // This is okay, we'll just... not use the BPF filter
        }

        Ok(out)
    }

    /// Get a new udev event
    ///
    /// If this returns Some, that's the event (singular) we managed to get.
    /// If this returns None, we have nothing more available now
    /// (try again later after epoll?)
    ///
    /// It may be useful to wrap this in another loop externally to dequeue *all*
    /// events which are currently available
    pub fn get_event(&self) -> io::Result<Option<HashMap<Vec<u8>, Option<Vec<u8>>>>> {
        loop {
            let pkt_sz = unsafe {
                libc::recv(
                    self.fd,
                    ptr::null_mut(),
                    0,
                    libc::MSG_PEEK | libc::MSG_TRUNC | libc::MSG_DONTWAIT,
                )
            };
            if pkt_sz < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Ok(None);
                }

                log::warn!("recv(peeking) failed on udev socket!");
                return Err(err);
            }
            let pkt_sz = pkt_sz as usize;

            let buf_layout =
                Layout::from_size_align(pkt_sz, mem::align_of::<UdevFeedcafeMessageHeader>())
                    .unwrap();
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
            if msg.msg_flags & libc::MSG_TRUNC != 0 {
                log::warn!("recv() got a truncated message on udev socket!");
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

            // We perform these checks *even though* some of them are redundant with BPF
            // (since we also don't hard-require BPF to work)
            pkt.mangle_and_validate().map_err(|_| {
                log::warn!("got a malformed udev packet!");
                io::Error::from(io::ErrorKind::Other)
            })?;
            if pkt.hdr.subsystem_hash != murmur_hash_2(b"usb", 0)
                || pkt.hdr.devtype_hash != murmur_hash_2(b"usb_device", 0)
            {
                continue;
            }
            let props = pkt
                .properties()
                .map(|x| (x.key.to_owned(), x.val.map(|x| x.to_owned())))
                .collect::<HashMap<_, _>>();
            if let Some(Some(subsystem)) = props.get(b"SUBSYSTEM".as_slice()) {
                if subsystem != b"usb" {
                    continue;
                }
            } else {
                continue;
            }
            if let Some(Some(devtype)) = props.get(b"DEVTYPE".as_slice()) {
                if devtype != b"usb_device" {
                    continue;
                }
            } else {
                continue;
            }

            // We are hardcoded to look for "add" actions
            if let Some(Some(action)) = props.get(b"ACTION".as_slice()) {
                if action != b"add" {
                    continue;
                }
            } else {
                continue;
            }

            return Ok(Some(props));
        }
    }
}
impl Drop for UdevNetlinkSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}
