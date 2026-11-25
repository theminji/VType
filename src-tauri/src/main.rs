// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if let [_, flag, pid] = std::env::args().collect::<Vec<_>>().as_slice() {
        if flag == "--toggle" {
            if let Err(err) = pid
                .parse()
                .map_err(|_| "Invalid VType process ID".to_string())
                .and_then(vtype_lib::toggle)
            {
                eprintln!("{err}");
                std::process::exit(1);
            }
            return;
        }
    }
    vtype_lib::run()
}
