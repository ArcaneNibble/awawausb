//! Unix-specific handling of stdio, in order to intentionally bypass Rust checks
//!
//! It is especially important on Windows that we are able to write binary data instead of text.
//!
//! Handling stdin requires a separate thread. See `architecture.md` for more information.

use std::cell::RefCell;
use std::io;
use std::ptr;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::Threading::*;

/// stdin reader thread function
fn stdin_read_thread(event: usize, tx: mpsc::Sender<io::Result<Vec<u8>>>) {
    let event = event as HANDLE;
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) };

    loop {
        // Read 4-byte length
        let mut msglen = [0u8; 4];
        let mut nbytes = 0;
        let ret = unsafe { ReadFile(stdin, msglen.as_mut_ptr(), 4, &mut nbytes, ptr::null_mut()) };
        if ret == 0 {
            let err = io::Error::last_os_error();
            let e = if err.kind() == io::ErrorKind::BrokenPipe {
                tx.send(Err(io::Error::from(io::ErrorKind::UnexpectedEof)))
            } else {
                tx.send(Err(err))
            };
            if e.is_err() {
                break;
            }
        }
        if nbytes < 4 {
            let e = tx.send(Err(io::Error::from(io::ErrorKind::UnexpectedEof)));
            if e.is_err() {
                break;
            }
        }
        let msglen = u32::from_ne_bytes(msglen);

        // Read actual message
        let mut buf: Vec<u8> = Vec::with_capacity(msglen as usize);
        let mut nbytes = 0;
        let ret = unsafe {
            ReadFile(
                stdin,
                buf.as_mut_ptr(),
                msglen,
                &mut nbytes,
                ptr::null_mut(),
            )
        };
        if ret == 0 {
            let e = tx.send(Err(io::Error::last_os_error()));
            if e.is_err() {
                break;
            }
        }
        if nbytes < msglen {
            let e = tx.send(Err(io::Error::from(io::ErrorKind::UnexpectedEof)));
            if e.is_err() {
                break;
            }
        }
        unsafe {
            buf.set_len(msglen as usize);
        }

        let e = tx.send(Ok(buf));
        if e.is_err() {
            break;
        }
        // SAFETY FIXME: We can race on this being torn down, but eh
        unsafe { SetEvent(event) };
    }

    log::debug!("Bye from stdin reading thread!");
}

/// State for the stdin reader thread
///
/// Note that this implements a hack to implement "peek" functionality.
#[derive(Debug)]
pub struct WinStdinReader {
    rx: mpsc::Receiver<io::Result<Vec<u8>>>,
    pub event: HANDLE,
    _peeked_message: RefCell<Option<io::Result<Vec<u8>>>>,
}
impl WinStdinReader {
    pub fn new() -> Self {
        let event = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        if event.is_null() {
            panic!(
                "Failed to set up stdin event! {}",
                io::Error::last_os_error()
            );
        }
        let event2 = event.addr();

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || stdin_read_thread(event2, tx));

        Self {
            rx,
            event,
            _peeked_message: RefCell::new(None),
        }
    }

    /// Check if there are any more messages on stdin
    pub fn peek_stdin(&self) -> bool {
        let mut peeked_message = self._peeked_message.borrow_mut();
        assert!(peeked_message.is_none(), "peeking while we have a message!");
        match self.rx.try_recv() {
            Ok(data) => {
                *peeked_message = Some(data);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                *peeked_message = Some(Err(io::Error::from(io::ErrorKind::UnexpectedEof)));
                true
            }
        }
    }

    /// Reads a message from stdin
    pub fn read_stdin_msg(&self) -> io::Result<Vec<u8>> {
        self._peeked_message.take().expect("didn't peek messages!")
    }
}
impl Drop for WinStdinReader {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.event);
        }
    }
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
