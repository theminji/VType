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

pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<bool, String> {
    use gtk::prelude::*;
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if !active() || !gtk_layer_shell::is_supported() {
        return Ok(false);
    }
    let window = window.gtk_window().map_err(|err| err.to_string())?;
    if window.is_realized() {
        return Err("The overlay window was realized before layer-shell setup.".into());
    }
    window.init_layer_shell();
    window.set_namespace("vtype-dictation");
    window.set_layer(Layer::Overlay);
    window.set_anchor(Edge::Bottom, true);
    window.set_layer_shell_margin(Edge::Bottom, 12);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);
    fn apply_input_shape(window: &gtk::ApplicationWindow, width: i32, height: i32) {
        if let Some(surface) = window.window() {
            // Only the pill intercepts clicks. GTK may allocate a larger webview.
            let hitbox = gtk::cairo::RectangleInt::new(
                (width - 88).max(0) / 2,
                (height - 50).max(0),
                88,
                38,
            );
            let region = gtk::cairo::Region::create_rectangle(&hitbox);
            surface.input_shape_combine_region(&region, 0, 0);
        }
    }
    window.connect_size_allocate(|window, bounds| {
        apply_input_shape(window, bounds.width(), bounds.height());
    });
    window.connect_map(|window| {
        let bounds = window.allocation();
        apply_input_shape(window, bounds.width(), bounds.height());
    });
    Ok(true)
}
