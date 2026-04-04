use std::cell::RefCell;
use std::convert::Infallible;
use std::io;
use std::ptr;

use core_foundation::base::CFRetain;
use kqueue_sys::*;

mod macos_sys;
mod stdio_unix;

use macos_sys::*;

/// Main struct holding all of the state for our operations
pub struct USBStubEngine {
    kqueue: i32,
    kevents_buf: RefCell<Vec<kevent>>,
    // The following only need to be held on to, we don't touch them
    _io_notification_port: *mut IONotificationPort,
    _plug_notifications: io_object_t,
    _unplug_notifications: io_object_t,
}
impl USBStubEngine {
    // XXX: Work around rustfmt not wanting to work otherwise??
    fn init_real(
        mut this: pin_init::PinUninit<'_, Self>,
    ) -> pin_init::InitResult<'_, Self, Infallible> {
        let v = this.get_mut().as_mut_ptr();

        // Create kqueue fd
        let kq = unsafe { kqueue() };

        // Set up a kevent for stdin
        let kevent_stdin = kevent::new(
            0,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
        );

        // Set up IOKit notifications incl. registering it for kevent
        let io_notif_port = unsafe { IONotificationPortCreate(0) };
        assert!(
            !io_notif_port.is_null(),
            "failed to create io notification port"
        );
        let io_notif_mach = unsafe { IONotificationPortGetMachPort(io_notif_port) };
        assert_ne!(
            io_notif_mach, 0,
            "failed to create io notification mach port"
        );

        let kevent_mach = kqueue_sys::kevent::new(
            io_notif_mach as usize,
            EventFilter::EVFILT_MACHPORT,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
        );

        let all_kevents = [kevent_stdin, kevent_mach];
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

        // Register IOKit notifications
        let matching = unsafe { IOServiceMatching(b"IOUSBDevice\0".as_ptr()) };
        // SAFETY: We add two matching notifications, so bump refcount by 1
        unsafe { CFRetain(matching as *const _) };

        let mut plug_notifications = io_object_t(0);
        let ret = unsafe {
            IOServiceAddMatchingNotification(
                io_notif_port,
                b"IOServiceFirstMatch\0".as_ptr(),
                matching,
                Self::iokit_plug_cb,
                v as *const (),
                &mut plug_notifications,
            )
        };
        assert_eq!(ret, 0, "failed to watch USB hotplug notifications");

        let mut unplug_notifications = io_object_t(0);
        let ret = unsafe {
            IOServiceAddMatchingNotification(
                io_notif_port,
                b"IOServiceTerminate\0".as_ptr(),
                matching,
                Self::iokit_unplug_cb,
                v as *const (),
                &mut unplug_notifications,
            )
        };
        assert_eq!(ret, 0, "failed to watch USB hotplug unplug notifications");

        // SAFETY: Make sure we set everything
        unsafe {
            (*v).kqueue = kq;
            // SAFETY: Don't drop uninit buf
            // (but others are okay, no drop impl)
            ptr::addr_of_mut!((*v).kevents_buf).write(RefCell::new(kevents_buf));
            (*v)._io_notification_port = io_notif_port;
            (*v)._plug_notifications = plug_notifications;
            (*v)._unplug_notifications = unplug_notifications;
        }

        // Invoke iteration once to capture everything pre-existing
        Self::iokit_plug_cb(v as *const (), plug_notifications);
        Self::iokit_unplug_cb(v as *const (), unplug_notifications);

        // SAFETY: Make sure we set everything
        unsafe { Ok(this.init_ok()) }
    }

    pub fn init() -> impl pin_init::Init<Self, Infallible> {
        pin_init::init_from_closure(Self::init_real)
    }

    /// Run one loop. Returns true if we should continue
    pub fn run_loop(&self) -> bool {
        // Poll for events
        let mut kevents_buf = self.kevents_buf.borrow_mut();
        unsafe {
            let nevents = kqueue_sys::kevent(
                self.kqueue,
                ptr::null(),
                0,
                kevents_buf.as_mut_ptr(),
                kevents_buf.capacity() as i32,
                ptr::null(),
            );

            if nevents < 0 {
                panic!("kevent poll failed: {:?}", std::io::Error::last_os_error());
            }

            // SAFETY: Set length to actual number of events
            kevents_buf.set_len(nevents as usize);
        };

        for kevent in kevents_buf.iter() {
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

    extern "C" fn iokit_plug_cb(self_: *const (), iterator: io_object_t) {
        dbg!("plug!");
        // SAFETY: We passed in self as the arg
        let self_ = unsafe { &*(self_ as *const Self) };
        dbg!(self_.kqueue);

        let mut item;
        loop {
            item = unsafe { IOIteratorNext(iterator) };
            if item.0 == 0 {
                break;
            }
        }
    }

    extern "C" fn iokit_unplug_cb(self_: *const (), iterator: io_object_t) {
        dbg!("unplug!");
        // SAFETY: We passed in self as the arg
        let self_ = unsafe { &*(self_ as *const Self) };
        dbg!(self_.kqueue);

        let mut item;
        loop {
            item = unsafe { IOIteratorNext(iterator) };
            if item.0 == 0 {
                break;
            }
        }
    }
}
impl Drop for USBStubEngine {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self._plug_notifications);
            IOObjectRelease(self._unplug_notifications);
            IONotificationPortDestroy(self._io_notification_port);
            libc::close(self.kqueue);
        }
    }
}

fn main() {
    eprintln!("Hello, world!");

    pin_init::init_stack!(state = USBStubEngine::init());
    let state = state.unwrap();
    // SAFETY: We need to *immediately* get rid of &mut,
    // because OS callbacks etc hold a reference to the engine object.
    // This is probably *still* incorrect, but dunno how to fix.
    let state = state.as_ref();
    while state.run_loop() {}

    eprintln!("zzz");
}
