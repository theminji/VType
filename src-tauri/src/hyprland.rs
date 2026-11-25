use std::io::ErrorKind;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;

static SOCKET: OnceLock<PathBuf> = OnceLock::new();

pub fn active() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn socket_path() -> Result<PathBuf, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(dir).join("vtype-dictation.sock"))
}

pub fn toggle() -> Result<(), String> {
    UnixDatagram::unbound()
        .and_then(|socket| socket.send_to(&[1], socket_path().map_err(std::io::Error::other)?))
        .map(|_| ())
        .map_err(|err| format!("Could not reach a running VType instance: {err}"))
}

pub fn setup(app: AppHandle) -> Result<(), String> {
    let path = socket_path()?;
    let socket = match UnixDatagram::bind(&path) {
        Ok(socket) => socket,
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            let probe = UnixDatagram::unbound().map_err(|err| err.to_string())?;
            match probe.connect(&path) {
                Err(err) if err.kind() == ErrorKind::ConnectionRefused => {
                    std::fs::remove_file(&path).map_err(|err| err.to_string())?;
                    UnixDatagram::bind(&path).map_err(|err| err.to_string())?
                }
                _ => return Err("Another VType instance is already listening.".into()),
            }
        }
        Err(err) => return Err(err.to_string()),
    };
    let _ = SOCKET.set(path);
    std::thread::spawn(move || {
        let mut buf = [0];
        while socket.recv(&mut buf).is_ok() {
            super::trigger_hotkey(&app);
        }
    });
    Ok(())
}

pub fn cleanup() {
    if let Some(path) = SOCKET.get() {
        let _ = std::fs::remove_file(path);
    }
}
