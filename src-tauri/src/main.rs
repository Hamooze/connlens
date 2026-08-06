// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = connlens_lib::cli::maybe_run(std::env::args().collect()) {
        std::process::exit(code);
    }

    connlens_lib::run()
}
