//! COM1 16550 UART serial driver.
//!
//! Phase 1 brings up a single serial console on COM1 (port 0x3F8) at
//! 115200 baud, 8 data bits, no parity, 1 stop bit (8N1). All output is
//! wrapped in a 'spin::Mutex' so early-boot logging from any core is
//! serialized safely.

use core::fmt::{self, Write};
use spin::Mutex;

/// I/O base address of COM1.
const COM1: u16 = 0x3F8;

/// Global singleton handle to the COM1 serial port.
pub static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1));

/// A thin wrapper around a 16550-compatible UART.
pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    pub const fn new(base: u16) -> Self {
        Self { base }
    }

    /// Configure the UART: disable IRQs, set 115200 8N1, enable FIFO.
    ///
    /// # Safety
    ///
    /// Issues raw 'out' instructions to I/O ports. Safe only when called
    /// from ring 0 with a real 16550 UART mapped at 'base'.
    pub fn init(&mut self) {
        // SAFETY: ring-0 early boot; COM1 base is a fixed ISA port.
        unsafe {
            self.write(1, 0x00); // Disable all interrupts

            self.write(3, 0x80); // Enable DLAB (set baud rate divisor)
            self.write(0, 0x01); // Divisor low  = 1  (115200 baud)
            self.write(1, 0x00); // Divisor high = 0

            self.write(3, 0x03); // 8 bits, no parity, one stop bit
            self.write(2, 0xC7); // Enable FIFO, clear, 14-byte threshold
            self.write(4, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    #[inline]
    unsafe fn write(&mut self, offset: u16, value: u8) {
        let port = self.base.wrapping_add(offset);
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags),
        );
    }

    #[inline]
    unsafe fn read(&mut self, offset: u16) -> u8 {
        let port = self.base.wrapping_add(offset);
        let value: u8;
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
        value
    }

    #[inline]
    fn wait_transmit_empty(&mut self) {
        // SAFETY: read of LSR (offset 5); safe in ring-0 early boot.
        while unsafe { self.read(5) } & 0x20 == 0 {
            core::hint::spin_loop();
        }
    }

    pub fn send(&mut self, byte: u8) {
        self.wait_transmit_empty();
        // SAFETY: THR write (offset 0); UART is initialized.
        unsafe { self.write(0, byte) };
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
            if byte == b'\n' {
                self.send(b'\r');
            }
        }
        Ok(())
    }
}

/// Initialize the global COM1 serial port. Call exactly once at boot.
pub fn init() {
    SERIAL.lock().init();
    let _ = SERIAL
        .lock()
        .write_str("\r\n[serial] COM1 online @ 115200 8N1\r\n");
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::serial::SERIAL.lock(), $($arg)*);
    }};
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\r\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\r\n", format_args!($($arg)*)));
}
