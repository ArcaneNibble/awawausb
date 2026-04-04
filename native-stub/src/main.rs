use std::{io, ptr};

use kqueue_sys::*;

mod stdio_unix;

fn main() {
    eprintln!("Hello, world!");

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

    let mut kevents_out_buf = Vec::with_capacity(all_kevents.len());

    'outer: loop {
        unsafe {
            let nevents = kqueue_sys::kevent(
                kq,
                ptr::null(),
                0,
                kevents_out_buf.as_mut_ptr(),
                kevents_out_buf.capacity() as i32,
                ptr::null(),
            );

            if nevents < 0 {
                panic!("kevent poll failed: {:?}", std::io::Error::last_os_error());
            }

            // SAFETY: Set length to actual number of events
            kevents_out_buf.set_len(nevents as usize);
        };

        for kevent in kevents_out_buf.iter() {
            if kevent.ident == 0 && kevent.filter == EventFilter::EVFILT_READ {
                let msg = stdio_unix::read_stdin_msg();
                if let Err(e) = &msg
                    && e.kind() == io::ErrorKind::UnexpectedEof
                {
                    eprintln!("Goodbye!");
                    break 'outer;
                }
                let msg = msg.expect("failed to read stdin");
                dbg!(&msg);
                stdio_unix::write_stdout_msg(&msg).expect("failed to write stdout");
            } else {
                dbg!(kevent);
            }
        }
    }

    eprintln!("zzz");
}
