# VType

<img width="128" height="128" alt="128x128" src="https://github.com/user-attachments/assets/f00ea938-4153-499e-8318-f887556607e9" />

## WORK IN PROGRESS

Voice typing without distractions

Default hotkey is `Ctrl/Cmd + Alt + r`. This will start recording (should show an icon) and then press it again to stop recording and transcribe

Only tested on Linux and Windows for now...

Linux and Windows binaries in [releases](https://github.com/theminji/VType/releases)

VType runs both models locally. On the first dictation it downloads
[Phonon-2.5](https://huggingface.co/FermionResearch/Phonon-2.5) (about 156 MB)
for speech recognition and [Spokenly Cleanup 1](https://huggingface.co/spokenly/cleanup-1)
(about 1.19 GB) to remove fillers and repetitions before typing. Later dictations
reuse the cached models. If cleanup fails or exceeds its context budget, VType
types the raw transcript.

Phonon recognizes 25 languages. Cleanup is trained for English, German, Spanish,
French, Chinese, Russian, and Japanese, with the strongest results in English.
VType currently applies cleanup to every transcript.

## Development setup

On Ubuntu, Pop!_OS, or Debian, install the WebKitGTK development package before
building. It also installs the JavaScriptCoreGTK development files that Cargo
looks up through `pkg-config`.

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev libxkbcommon-dev
```

For other Linux build dependencies and platforms, see the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

VType looks for a project `.venv` when launched from the repository. You can set
`VTYPE_PYTHON` to select a different Python executable.

Hyprland uses a compositor-managed shortcut. Add this to your Hyprland Lua
configuration and apply it through your usual configuration workflow. Use
the absolute path to your built VType executable when developing locally.

```lua
hl.bind("CTRL + ALT + R", hl.dsp.exec_cmd("vtype --toggle"))
hl.window_rule({
  match = { title = "^VType Dictation$" },
  float = true, no_initial_focus = true, no_focus = true,
  no_blur = true, no_shadow = true, no_anim = true,
  border_size = 0, rounding = 0,
  move = { "(monitor_w-window_w)/2", "monitor_h-window_h-24" },
})
```

VType never modifies or reloads the compositor configuration. Change the bind
in that configuration to choose a different shortcut on Hyprland. The settings
panel generates the Lua bind for the shortcut you choose.

VType ships with direct typing for Windows, macOS, Linux/X11, and Wayland
compositors that expose the virtual-keyboard protocol, including Hyprland.
macOS may require Accessibility permission. VType types line breaks and tabs
as spaces to avoid submitting text or changing focus.

```sh
python -m venv .venv
source .venv/bin/activate
python -m pip install -r requirements.txt
npm ci
npm run tauri dev
```

To use an existing unpacked Phonon-2.5 model directory, set `VTYPE_MODEL_PATH`
before launching VType.

## Settings

Start dictation, then double-click the pill to open **VType Settings**. This
discards the recording in progress. The tray menu also provides a Settings
entry. Save changes to update the shortcut, turn Spokenly formatting on or
off, give Phonon up to 25 names or phrases to listen for, adjust the pill's
sensitivity, or hide the completion confirmation.

## Install and uninstall

The **Desktop installers** workflow builds artifacts for Windows x64, macOS
Intel and Apple silicon, and Linux x64. Download the artifact for your platform
from the workflow run. To build packages locally, install the development
prerequisites and make `uv` available on the build machine, then run
`npm ci` and `npm run installer`. The packages are under
`src-tauri/target/release/bundle`.

| Platform | Install | Uninstall |
| --- | --- | --- |
| Windows | Run the NSIS setup executable. | Choose VType in **Settings → Apps → Installed apps → Uninstall**. |
| macOS | Open the DMG and drag VType to Applications. | Quit VType and move it from Applications to Trash. |
| Debian / Ubuntu | Run `sudo apt install ./vtype_<version>_amd64.deb`. | Run `sudo apt remove vtype`. |
| Fedora | Run `sudo dnf install ./vtype-<version>-1.x86_64.rpm`. | Run `sudo dnf remove vtype`. |

The workflow produces unsigned packages unless signing is configured on the
build machine. Windows and macOS may flag an unsigned download. On macOS,
VType needs Microphone and Accessibility permissions to record and type.

Installed copies set up their own Python environment and download speech
dependencies on first dictation. An internet connection is required for this
setup and the initial model downloads. The formatter model downloads only
when formatting is enabled and there is text to clean up.

Native uninstall removes the application and retains preferences and downloaded
models. To remove those too, quit VType and delete its `com.maria.vtype`
directories from the following locations.

| Platform | Preferences and runtime data locations |
| --- | --- |
| Windows | `%APPDATA%` and `%LOCALAPPDATA%` |
| macOS | `~/Library/Application Support/` |
| Linux | `~/.config/` and `~/.local/share/`, or the XDG config and data directories you configured |
