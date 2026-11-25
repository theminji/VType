// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--toggle") {
        if let Err(err) = vtype_lib::toggle() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    vtype_lib::run()
}
