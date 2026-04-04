use std::{io, ptr};

use kqueue_sys::*;

mod stdio_unix;

/// Main struct holding all of the state for our operations
pub struct USBStubEngine {
    kqueue: i32,
    kevents_buf: Vec<kevent>,
}
impl USBStubEngine {
    pub fn init() -> Self {
        // Create kqueue fd
        let kq = unsafe { kqueue() };

        // Set up a kevent for stdin
        let kevent_stdin = kevent::new(
            0,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
        );

        let all_kevents = [kevent_stdin];
        if unsafe {
            kqueue_sys::kevent(
                kq,
                all_kevents.as_ptr(),
                all_kevents.len() as i32,
                ptr::null_mut(),
                0,
                ptr::null(),
            )
        } < 0
        {
            panic!("kevent add failed: {:?}", std::io::Error::last_os_error());
        }

        // Prepare buffer for reading events
        let kevents_buf = Vec::with_capacity(all_kevents.len());

        Self {
            kqueue: kq,
            kevents_buf,
        }
    }

    /// Run one loop. Returns true if we should continue
    pub fn run_loop(&mut self) -> bool {
        // Poll for events
        unsafe {
            let nevents = kqueue_sys::kevent(
                self.kqueue,
                ptr::null(),
                0,
                self.kevents_buf.as_mut_ptr(),
                self.kevents_buf.capacity() as i32,
                ptr::null(),
            );

            if nevents < 0 {
                panic!("kevent poll failed: {:?}", std::io::Error::last_os_error());
            }

            // SAFETY: Set length to actual number of events
            self.kevents_buf.set_len(nevents as usize);
        };

        for kevent in self.kevents_buf.iter() {
            if kevent.ident == 0 && kevent.filter == EventFilter::EVFILT_READ {
                let msg = stdio_unix::read_stdin_msg();
                if let Err(e) = &msg
                    && e.kind() == io::ErrorKind::UnexpectedEof
                {
                    eprintln!("Goodbye!");
                    return false;
                }
                let msg = msg.expect("failed to read stdin");

                // TODO: Implement stdin handling
                dbg!(&msg);
                stdio_unix::write_stdout_msg(&msg).expect("failed to write stdout");
            } else {
                dbg!(kevent);
            }
        }

        true
    }
}

fn main() {
    eprintln!("Hello, world!");

    let mut state = USBStubEngine::init();
    while state.run_loop() {}

    eprintln!("zzz");
}
