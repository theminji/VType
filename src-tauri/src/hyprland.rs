use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::Command;
use tauri::AppHandle;

const BINDING: &str = "CTRL + ALT + R";

pub fn active() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn socket_path(pid: u32) -> Result<PathBuf, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(dir).join(format!("vtype-{pid}.sock")))
}

pub fn toggle(pid: u32) -> Result<(), String> {
    UnixDatagram::unbound()
        .and_then(|socket| socket.send_to(&[1], socket_path(pid).map_err(std::io::Error::other)?))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn lua(command: &str, script: &str) -> Result<(), String> {
    let output = Command::new("hyprctl")
        .args([command, script])
        .output()
        .map_err(|err| err.to_string())?;
    let response = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && response.trim() == "ok" {
        return Ok(());
    }
    Err(format!(
        "hyprctl {command}: {} {}",
        response.trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

pub fn setup(app: AppHandle) -> Result<(), String> {
    let pid = std::process::id();
    let path = socket_path(pid)?;
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let exe = exe.to_str().ok_or("VType executable path must be UTF-8")?;
    let command = format!("'{}' --toggle {pid}", exe.replace('\'', "'\\''"));
    let command = serde_json::to_string(&command).map_err(|err| err.to_string())?;
    let socket = UnixDatagram::bind(&path).map_err(|err| err.to_string())?;
    let script = format!(
        r#"
        if vtype_binding then vtype_binding:remove() end
        vtype_overlay_rule = hl.window_rule({{
            name = "vtype-dictation-overlay",
            match = {{ title = "^VType Dictation$" }},
            float = true, no_initial_focus = true, no_focus = true,
            no_blur = true, no_dim = true, no_shadow = true, no_anim = true,
            border_size = 0, rounding = 0, opacity = "1.0 override 1.0 override",
            move = {{ "(monitor_w-window_w)/2", "monitor_h-window_h-24" }},
        }})
        vtype_binding = hl.bind("{BINDING}", hl.dsp.exec_cmd({command}))
        vtype_pid = {pid}
        "#
    );
    if let Err(err) = lua("eval", &script) {
        let _ = std::fs::remove_file(path);
        return Err(err);
    }
    std::thread::spawn(move || {
        let mut buf = [0];
        while socket.recv(&mut buf).is_ok() {
            super::trigger_hotkey(&app);
        }
    });
    Ok(())
}

pub fn cleanup() {
    let pid = std::process::id();
    if let Ok(path) = socket_path(pid) {
        if std::fs::remove_file(path).is_ok() {
            let _ = lua(
                "eval",
                &format!(
                    r#"
                if vtype_pid == {pid} then
                    if vtype_binding then vtype_binding:remove(); vtype_binding = nil end
                    if vtype_overlay_rule then vtype_overlay_rule:set_enabled(false) end
                end
                "#
                ),
            );
        }
    }
}
