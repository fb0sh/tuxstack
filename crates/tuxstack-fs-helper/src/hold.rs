//! `hold` command: container PID 1 keep-alive.
//!
//! Must be a good citizen: handle SIGTERM (exit cleanly), never busy-poll,
//! produce no log noise, open no sockets, and never touch the browsed
//! filesystem. The daemon force-removes the container (SIGKILL) on teardown,
//! so this is only about graceful shutdown when something sends SIGTERM.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static STOPPING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    STOPPING.store(true, Ordering::SeqCst);
}

pub fn run() -> ! {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
    loop {
        if STOPPING.load(Ordering::SeqCst) {
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
