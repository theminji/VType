use serde::{Deserialize, Serialize};
use std::{collections::HashMap, process::Command, thread, time::Duration};
use wayland_client::{
    backend::ObjectId,
    delegate_noop, event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1 as handle, zwlr_foreign_toplevel_manager_v1 as manager,
};

#[derive(Clone, Deserialize, Serialize)]
pub struct FocusedWindow {
    address: String,
    pid: u32,
    class: String,
    title: String,
}

impl FocusedWindow {
    fn is_same(&self, other: &Self) -> bool {
        self.address == other.address && self.pid == other.pid
    }

    fn is_external(&self) -> bool {
        self.pid != 0 && self.pid != std::process::id() && self.address.starts_with("0x")
    }
}

fn query<T: serde::de::DeserializeOwned>(command: &str) -> Result<T, String> {
    let output = Command::new("hyprctl")
        .args(["-j", command])
        .output()
        .map_err(|err| format!("Could not read Hyprland window state: {err}"))?;
    if !output.status.success() {
        return Err("Could not read Hyprland window state.".into());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "Hyprland did not return a focused window.".into())
}

pub fn capture() -> Option<FocusedWindow> {
    query::<FocusedWindow>("activewindow")
        .ok()
        .filter(FocusedWindow::is_external)
}

pub fn restore(target: &FocusedWindow) -> Result<(), String> {
    if !target.is_external() {
        return Err("The dictation target is unavailable.".into());
    }
    if capture().is_some_and(|active| active.is_same(target)) {
        return Ok(());
    }
    let windows: Vec<FocusedWindow> = query("clients")?;
    let current = windows
        .iter()
        .find(|window| window.is_same(target))
        .ok_or("The dictation window was closed.")?;
    activate(current)?;
    for _ in 0..15 {
        if capture().is_some_and(|active| active.is_same(target)) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(30));
    }
    Err("Hyprland did not focus the dictation window.".into())
}

#[derive(Default)]
struct Toplevels {
    windows: HashMap<ObjectId, Toplevel>,
}

struct Toplevel {
    handle: handle::ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
}

fn activate(target: &FocusedWindow) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|err| err.to_string())?;
    let (globals, mut queue) =
        registry_queue_init::<Toplevels>(&connection).map_err(|err| err.to_string())?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals
        .bind(&qh, 1..=8, ())
        .map_err(|err| err.to_string())?;
    let _manager: manager::ZwlrForeignToplevelManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .map_err(|err| err.to_string())?;
    let mut state = Toplevels::default();
    queue.roundtrip(&mut state).map_err(|err| err.to_string())?;

    let mut matches = state.windows.values().filter(|window| {
        window.title == target.title && window.app_id.eq_ignore_ascii_case(&target.class)
    });
    let selected = matches
        .next()
        .ok_or("Could not identify the dictation window on Wayland.")?;
    if matches.next().is_some() {
        return Err("Multiple windows match the dictation target.".into());
    }
    selected.handle.activate(&seat);
    queue.roundtrip(&mut state).map_err(|err| err.to_string())?;
    Ok(())
}

delegate_noop!(Toplevels: ignore wl_seat::WlSeat);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Toplevels {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<manager::ZwlrForeignToplevelManagerV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        _: &manager::ZwlrForeignToplevelManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let manager::Event::Toplevel { toplevel } = event {
            state.windows.insert(
                toplevel.id(),
                Toplevel {
                    handle: toplevel,
                    title: String::new(),
                    app_id: String::new(),
                },
            );
        }
    }

    event_created_child!(Toplevels, manager::ZwlrForeignToplevelManagerV1, [
        0 => (handle::ZwlrForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<handle::ZwlrForeignToplevelHandleV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        handle: &handle::ZwlrForeignToplevelHandleV1,
        event: handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            handle::Event::Closed => {
                state.windows.remove(&handle.id());
                handle.destroy();
            }
            event => {
                if let Some(window) = state.windows.get_mut(&handle.id()) {
                    match event {
                        handle::Event::Title { title } => window.title = title,
                        handle::Event::AppId { app_id } => window.app_id = app_id,
                        _ => {}
                    }
                }
            }
        }
    }
}
