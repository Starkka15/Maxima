use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use log::{Level, LevelFilter, Metadata, Record};

pub struct SimpleLogger;
pub static LOGGER: SimpleLogger = SimpleLogger;

/// Optional file sink. Opened by `init_logger` and written to in addition to
/// stdout. Always-on so logs survive even when the binary is invoked from a
/// GUI parent (e.g. maxima-bootstrap inside Wine) that didn't allocate a
/// console — `println!` output would otherwise vanish.
///
/// Failure to open the file is non-fatal: the logger silently falls back to
/// stdout-only.
static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

/// When true, the logger writes ONLY to the file sink — stdout stays clean.
/// Used by `--json` subcommands so callers (Draconis, scripts) can parse
/// stdout as a single JSON document without log noise. The file sink keeps
/// receiving everything so debugging isn't affected.
static SUPPRESS_STDOUT: AtomicBool = AtomicBool::new(false);

/// Toggle stdout output from the logger. File sink is unaffected. Call this
/// after `Args::parse()` and before any heavy logging when entering a
/// `--json` code path.
pub fn set_stdout_suppressed(suppressed: bool) {
    SUPPRESS_STDOUT.store(suppressed, Ordering::Relaxed);
}

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let enable_debug = if let Ok(x) = env::var("MAXIMA_LOG_LEVEL") {
            x == "debug"
        } else {
            false
        };

        let log_level = if enable_debug {
            Level::Debug
        } else {
            Level::Info
        };

        metadata.level() <= log_level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let level = record.level();
        let color: &str = match level {
            Level::Error => "31", // red
            Level::Warn => "33",  // yellow
            Level::Info => "32",  // green
            Level::Debug => "36", // cyan
            Level::Trace => "33", // yellow
        };

        let (colored, plain) = if level == Level::Error {
            (
                format!(
                    "\u{001b}[{}m{}\u{001b}[37m - [{}:{}] - {}",
                    color,
                    level,
                    record.file_static().unwrap_or("?"),
                    record.line().unwrap_or(0),
                    record.args()
                ),
                format!(
                    "{} - [{}:{}] - {}",
                    level,
                    record.file_static().unwrap_or("?"),
                    record.line().unwrap_or(0),
                    record.args()
                ),
            )
        } else {
            (
                format!(
                    "\u{001b}[{}m{}\u{001b}[37m - [{}] - {}",
                    color,
                    level,
                    record.module_path().unwrap_or("?"),
                    record.args()
                ),
                format!(
                    "{} - [{}] - {}",
                    level,
                    record.module_path().unwrap_or("?"),
                    record.args()
                ),
            )
        };

        // stdout — visible when a console is attached. Skipped in JSON
        // subcommand modes so the caller's stdout parser doesn't choke on
        // interleaved log lines.
        if !SUPPRESS_STDOUT.load(Ordering::Relaxed) {
            println!("{}", colored);
        }

        // File sink — best-effort. We don't want logging to ever panic.
        //
        // Flushing every line would be wasteful for high-volume Info/Debug
        // output (Gemini caught this in code review). But for Warn/Error we
        // *do* want the line on disk before any subsequent crash: those are
        // exactly the events you read the file to diagnose. So: flush on
        // Warn/Error, rely on stdio buffering otherwise. The explicit
        // `Log::flush()` impl below covers the normal shutdown path.
        if let Ok(mut guard) = LOG_FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{}", plain);
                if matches!(level, Level::Warn | Level::Error) {
                    let _ = file.flush();
                }
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = LOG_FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// Resolves where to write the log file. Precedence:
/// 1. `$MAXIMA_LOG_FILE` (explicit override — must be an absolute path).
/// 2. Per-OS sensible default under a `Maxima/Logs` namespace.
/// 3. `None` if no writable location can be determined.
fn resolve_log_file_path(binary_name: &str) -> Option<PathBuf> {
    if let Ok(p) = env::var("MAXIMA_LOG_FILE") {
        return Some(PathBuf::from(p));
    }

    let dir: Option<PathBuf>;
    #[cfg(windows)]
    {
        // %LOCALAPPDATA%\Maxima\Logs (per-user, always writable in a CrossOver
        // bottle's drive_c/users/<user>/AppData/Local).
        dir = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| env::var_os("APPDATA").map(PathBuf::from))
            .map(|p| p.join("Maxima").join("Logs"));
    }
    #[cfg(unix)]
    {
        // $XDG_DATA_HOME/maxima/logs or ~/.local/share/maxima/logs
        dir = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
            .map(|p| p.join("maxima").join("logs"));
    }

    dir.map(|d| d.join(format!("{}.log", binary_name)))
}

/// Initialize the logger. `binary_name` is used to derive the default log file
/// name (e.g. `"maxima-cli"` → `maxima-cli.log`).
pub fn init_logger_named(binary_name: &str) {
    if enable_ansi_support::enable_ansi_support().is_err() {
        // stderr — avoids corrupting stdout for `--json` callers in the rare
        // case where ANSI support negotiation fails before we know which
        // subcommand will run.
        eprintln!("ANSI Colors are unsupported in your terminal, things might look a bit off!");
    }

    // Best-effort file sink. Any failure here is silently swallowed — we still
    // have stdout. The whole point is to be a safety net, not a hard dep.
    if let Some(path) = resolve_log_file_path(binary_name) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(mut guard) = LOG_FILE.lock() {
                *guard = Some(file);
            }
            // Header line so distinct invocations are easy to skim.
            log_session_header(&path);
        }
    }

    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Trace))
        .ok();
}

/// Backwards-compatible entry point. Defaults to `maxima` as the log name.
pub fn init_logger() {
    init_logger_named("maxima");
}

fn log_session_header(path: &PathBuf) {
    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(
                file,
                "\n===== maxima log session opened (pid={}) — {} =====",
                std::process::id(),
                path.display()
            );
        }
    }
}
