use crate::{serial_print, serial_println};
use log::Log;

pub struct Logger;

impl Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool { true }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        serial_print!("[{}][{}]", record.level(), record.target());

        if let Some(file) = record.file() {
            serial_print!("[{}", file);
            if let Some(line) = record.line() {
                serial_print!(":{}", line);
            }
            serial_print!("]");
        }

        serial_println!("{}", record.args());
    }

    fn flush(&self) {}
}
