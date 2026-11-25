# VType

<img width="128" height="128" alt="128x128" src="https://github.com/user-attachments/assets/f00ea938-4153-499e-8318-f887556607e9" />

## WORK IN PROGRESS

Voice typing without distractions

Default hotkey is `Ctrl/Cmd + Alt + r`. This will start recording (should show an icon) and then press it again to stop recording and transcribe

Only tested on Linux  for now...

Install binaries in [releases](https://github.com/theminji/VType/releases)

VType runs both models locally. The setup window for installed copies installs
[Phonon-2.5](https://huggingface.co/FermionResearch/Phonon-2.5) (about 156 MB)
and [Spokenly Cleanup 1 Q8 GGUF](https://huggingface.co/spokenly/cleanup-1-gguf) (639 MB).
Later dictations reuse the installed models. If cleanup fails or exceeds its
context budget, VType types the raw transcript.

Phonon recognizes 25 languages. Cleanup is trained for English, German, Spanish,
French, Chinese, Russian, and Japanese, with the strongest results in English.
VType currently applies cleanup to every transcript.

## Development setup

On Ubuntu, Pop!_OS, or Debian, install the WebKitGTK development package before
building. It also installs the JavaScriptCoreGTK development files that Cargo
looks up through `pkg-config`.

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev libxkbcommon-dev libgtk-layer-shell-dev cmake clang libclang-dev
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
-- Optional when your other layer rules enable blur.
hl.layer_rule({ match = { namespace = "^vtype-dictation$" }, blur = false })
```

The pill uses a non-focusable Wayland layer on Hyprland. Dictation restores
the window that was active when recording started and checks focus before
typing. VType never modifies or reloads the compositor configuration. Change the bind
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

## Analytics and settings

Start dictation, then double-click the pill to open **Analytics & settings**.
This discards the recording in progress. You can also open the panel from the tray.

Analytics tracks recognized words, common English fillers, speaking pace, and
successful insertions. The time-saved estimate compares your typing speed
with the time spent recording, processing, and inserting text. VType stores
daily totals locally. It does not keep transcripts for analytics.

Open the **Settings** tab to adjust the typing-speed estimate. Save changes to update the shortcut, turn Spokenly formatting on or
off, give Phonon up to 25 names or phrases to listen for, adjust the pill's
sensitivity, or hide the completion confirmation.

## Install and uninstall

Download the installer for your platform from **Releases**. The **Desktop
installers** workflow builds packages for Windows x64, macOS Intel and Apple
silicon, and Linux x64.  To build packages locally, install the development
prerequisites and make `uv`, CMake, and Clang available on the build machine, then run
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

After installing, quit any older VType process and open VType from the app menu.
Choose **Install models** in the setup window. VType installs its Python
environment and both models before enabling dictation. Setup needs internet
access, and later dictation runs locally. The native package contains the
bootstrapper, and setup downloads the model files into your user account.
Run `vtype --setup` to reopen the setup window after completion.

Native uninstall removes the application and retains preferences and downloaded
models. To remove those too, quit VType and delete its `com.maria.vtype`
directories from the following locations.

| Platform | Preferences and runtime data locations |
| --- | --- |
| Windows | `%APPDATA%` and `%LOCALAPPDATA%` |
| macOS | `~/Library/Application Support/` |
| Linux | `~/.config/` and `~/.local/share/`, or the XDG config and data directories you configured |
