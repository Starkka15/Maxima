use anyhow::Result;
use log::{Record, Level, Metadata, LevelFilter};

pub struct SimpleLogger;
pub static LOGGER: SimpleLogger = SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let log_level = if cfg!(debug_assertions) {
            Level::Debug
        } else {
            Level::Info
        };

        metadata.level() <= log_level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let level = record.level();
            let color: &str = match level {
                Level::Error => "31", // red
                Level::Warn => "33", // yellow
                Level::Info => "32", // green
                Level::Debug => "36", // cyan
                Level::Trace => "33", // yellow
            };

            println!("\u{001b}[{}m{}\u{001b}[37m - [{}] - {}", color, level, record.module_path().unwrap(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init_logger() -> Result<()> {
    if enable_ansi_support::enable_ansi_support().is_err() {
        println!("ANSI Colors are unsupported in your terminal, things might look a bit off!");
    }

    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Trace))?;
    Ok(())
}