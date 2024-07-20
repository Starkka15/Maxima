use std::io;

use maxima_resources::maxima_windows_rc;

fn main() -> io::Result<()> {
    maxima_windows_rc("maximabootstrap", "Maxima Launcher")
}
