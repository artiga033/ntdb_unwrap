#![no_std]

mod offset_vfs;
pub use offset_vfs::*;

#[cfg(feature = "_cdylib")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    extern "C" {
        fn abort() -> !;
    }

    unsafe {
        abort();
    }
}
