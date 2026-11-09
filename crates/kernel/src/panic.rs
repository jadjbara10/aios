//! Kernel panic handler.

use core::fmt::Write;
use core::panic::PanicInfo;

use crate::serial;

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    let mut port = serial::SERIAL.lock();
    let _ = write_panic_banner(&mut *port, info);

    loop {
        // SAFETY: 'cli' disables interrupts, 'hlt' halts the core.
        unsafe {
            core::arch::asm!(
                "cli",
                "hlt",
                options(nostack, nomem, preserves_flags),
            );
        }
    }
}

fn write_panic_banner(
    port: &mut serial::SerialPort,
    info: &PanicInfo,
) -> fmt::Result {
    writeln!(port, "\r\n!!! KERNEL PANIC !!!")?;
    if let Some(loc) = info.location() {
        writeln!(
            port,
            "  at {}:{}:{}",
            loc.file(),
            loc.line(),
            loc.column()
        )?;
    }
    if let Some(msg) = info.message() {
        writeln!(port, "  msg: {}", msg)?;
    } else if let Some(payload) = info.payload().downcast_ref::<&str>() {
        writeln!(port, "  msg: {}", payload)?;
    }
    writeln!(port, "  CPU halted.")
}
