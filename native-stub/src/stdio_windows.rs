use std::io;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Console::*;

/// Reads a message from stdin
pub fn read_stdin_msg() -> io::Result<Vec<u8>> {
    todo!()
}

/// Writes a message to stdout
pub fn write_stdout_msg(data: &[u8]) -> io::Result<()> {
    let len = data.len();
    if len > 1 * 1024 * 1024 {
        panic!("message too long to send!");
    }

    // Unfortunately, Windows doesn't have something like writev
    // that works on free-size buffers. Gotta copy-pasta...
    let mut new_buf = Vec::with_capacity(len + 4);
    new_buf.extend_from_slice(&u32::to_ne_bytes(len as u32));
    new_buf.extend_from_slice(data);

    let mut wbytes = 0;
    let ret = unsafe {
        WriteFile(
            GetStdHandle(STD_OUTPUT_HANDLE),
            new_buf.as_ptr(),
            new_buf.len() as u32,
            &mut wbytes,
            ptr::null_mut(),
        )
    };
    if ret == 0 {
        return Err(io::Error::last_os_error());
    }
    if (wbytes as usize) != new_buf.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "stdout write truncated",
        ));
    }

    Ok(())
}
