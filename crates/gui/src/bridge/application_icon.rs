//! Installs the bundled TuxStack icon as Qt's application/window icon.

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("application_icon.h");

        fn set_tuxstack_application_icon() -> bool;
    }
}

pub fn install() -> bool {
    ffi::set_tuxstack_application_icon()
}
