use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tauri::{path::BaseDirectory, AppHandle, Manager};

const REQUIREMENTS: &str = include_str!("../../requirements.txt");
const PYTHON_VERSION: &str = "3.12";

pub fn bundled_uv(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let executable = if cfg!(windows) { "uv.exe" } else { "uv" };
    let uv = app
        .path()
        .resolve(format!("runtime/{executable}"), BaseDirectory::Resource)
        .map_err(|err| err.to_string())?;
    Ok(uv.is_file().then_some(uv))
}

pub fn bundled_python(app: &AppHandle) -> Result<Option<Command>, String> {
    let Some(uv) = bundled_uv(app)? else {
        return Ok(None);
    };

    let data = app
        .path()
        .app_local_data_dir()
        .map_err(|err| err.to_string())?;
    let runtime = data.join("runtime");
    let environment = runtime.join("venv");
    let python = environment.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    let marker = runtime.join("installed-requirements.txt");
    if !python.is_file() || fs::read_to_string(&marker).ok().as_deref() != Some(REQUIREMENTS) {
        fs::create_dir_all(&runtime).map_err(|err| err.to_string())?;
        let requirements = runtime.join("requirements.txt");
        fs::write(&requirements, REQUIREMENTS).map_err(|err| err.to_string())?;
        if !python.is_file() {
            let mut setup = uv_command(&uv, &runtime);
            setup
                .args([
                    "venv",
                    "--no-project",
                    "--managed-python",
                    "--python",
                    PYTHON_VERSION,
                ])
                .arg(&environment);
            run(setup, "prepare Python")?;
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if cfg!(target_arch = "x86_64") {
            let mut setup = uv_command(&uv, &runtime);
            setup
                .args(["pip", "install", "--python"])
                .arg(&python)
                .args(["torch", "--index", "https://download.pytorch.org/whl/cpu"]);
            run(setup, "install the CPU runtime")?;
        }
        let mut setup = uv_command(&uv, &runtime);
        setup
            .args(["pip", "install", "--python"])
            .arg(&python)
            .arg("-r")
            .arg(requirements);
        run(setup, "install speech dependencies")?;
        fs::write(&marker, REQUIREMENTS).map_err(|err| err.to_string())?;
    }

    let mut command = Command::new(python);
    if std::env::var_os("HF_HOME").is_none() {
        command.env("HF_HOME", data.join("huggingface"));
    }
    if std::env::var_os("FERMION_CACHE_DIR").is_none() {
        command.env("FERMION_CACHE_DIR", data.join("fermion"));
    }
    Ok(Some(command))
}

fn uv_command(uv: &Path, runtime: &Path) -> Command {
    let mut command = Command::new(uv);
    command
        .current_dir(runtime)
        .env("UV_PYTHON_INSTALL_DIR", runtime.join("python"))
        .env("UV_CACHE_DIR", runtime.join("cache"))
        .env("UV_NO_PROGRESS", "1")
        .env("UV_NO_CONFIG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    super::configure_background_command(&mut command);
    command
}

fn run(mut command: Command, task: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|err| format!("Could not {task}: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "Could not {task}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}
