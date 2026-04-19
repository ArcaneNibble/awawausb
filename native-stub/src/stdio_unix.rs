//! Unix-specific handling of stdio, in order to intentionally bypass Rust checks
//!
//! It is not clear the extent to which this is necessary,
//! but our IO is performed totally unbuffered.
//! We also write binary data rather than UTF-8 or "text".
//!
//! Writing is also performed using iovec, so that the length and payload are written
//! as a single atomic chunk.

use std::io;

/// Reads a message from stdin
pub fn read_stdin_msg() -> io::Result<Vec<u8>> {
    // Read 4-byte length
    let mut msglen = [0u8; 4];
    let nbytes = unsafe { libc::read(0, msglen.as_mut_ptr() as *mut _, 4) };
    if nbytes < 0 {
        return Err(io::Error::last_os_error());
    }
    if nbytes < 4 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    let msglen = u32::from_ne_bytes(msglen) as usize;

    // Read actual message
    let mut buf: Vec<u8> = Vec::with_capacity(msglen);
    let nbytes = unsafe { libc::read(0, buf.as_mut_ptr() as *mut _, msglen) };
    if nbytes < 0 {
        return Err(io::Error::last_os_error());
    }
    if (nbytes as usize) < msglen {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    unsafe {
        buf.set_len(msglen);
    }

    Ok(buf)
}

/// Writes a message to stdout
pub fn write_stdout_msg(data: &[u8]) -> io::Result<()> {
    let len = data.len();
    if len > 1 * 1024 * 1024 {
        panic!("message too long to send!");
    }

    let len = u32::to_ne_bytes(len as u32);
    let iov = [
        libc::iovec {
            iov_base: len.as_ptr() as *mut _,
            iov_len: 4,
        },
        libc::iovec {
            iov_base: data.as_ptr() as *mut _,
            iov_len: data.len(),
        },
    ];

    let wbytes = unsafe { libc::writev(1, iov.as_ptr(), 2) };
    if wbytes < 0 {
        return Err(io::Error::last_os_error());
    }
    if (wbytes as usize) != data.len() + 4 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "stdout write truncated",
        ));
    }

    Ok(())
}
