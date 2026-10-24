// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The bundled long-running diagnostic runs this same executable with a
    // flag, so it needs no external tool and cannot run arbitrary code.
    if std::env::args().nth(1).as_deref() == Some(shiftops_desktop_lib::DIAGNOSTIC_TICKER_FLAG) {
        shiftops_desktop_lib::run_diagnostic_ticker();
        return;
    }
    shiftops_desktop_lib::run()
}
