use core::fmt::Arguments;
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        let domain_id = shared_heap::domain_id();
        let mut id: usize;
        unsafe {
            core::arch::asm!(
            "mv {},tp", out(reg)id,
            );
        }
        id = id as u32 as usize;
        $crate::console::__print(format_args!("[{}][Domain:{}] {}", id,domain_id, format_args!($($arg)*)))
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($fmt:expr) => ($crate::print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print!(
        concat!($fmt, "\n"), $($arg)*));
}

/// Print with color
///
/// The first argument is the color, which should be one of the following:
/// - 30: Black
/// - 31: Red
/// - 32: Green
/// - 33: Yellow
/// - 34: Blue
/// - 35: Magenta
/// - 36: Cyan
/// - 37: White
///
#[macro_export]
macro_rules! println_color {
    ($color:expr, $fmt:expr) => {
        $crate::print!(concat!("\x1b[", $color, "m", $fmt, "\x1b[0m\n"));
    };
    ($color:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!("\x1b[", $color, "m", $fmt, "\x1b[0m\n"), $($arg)*);
    };
}

pub fn __print(args: Arguments) {
    // Format the whole line first. The kernel serializes each write_console
    // call, so this keeps a log line from interleaving with output from
    // another domain or hart.
    let line = alloc::format!("{}", args);
    corelib::write_console(&line);
}
