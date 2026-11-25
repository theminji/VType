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
(about 1.19 GB) to remove fillers and repetitions before pasting. Later dictations
reuse the cached models. If cleanup fails or exceeds its context budget, VType
pastes the raw transcript.

Phonon recognizes 25 languages. Cleanup is trained for English, German, Spanish,
French, Chinese, Russian, and Japanese, with the strongest results in English.
VType currently applies cleanup to every transcript.

## Development setup

On Ubuntu, Pop!_OS, or Debian, install the WebKitGTK development package before
building. It also installs the JavaScriptCoreGTK development files that Cargo
looks up through `pkg-config`.

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev
```

For other Linux build dependencies and platforms, see the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

VType looks for a project `.venv` when launched from the repository. You can set
`VTYPE_PYTHON` to select a different Python executable.

On Hyprland 0.55 and newer, VType registers Ctrl+Alt+R through Hyprland and
uses its paste dispatcher. Restart VType after reloading your Hyprland config
to restore the shortcut and overlay rule.

```sh
python -m venv .venv
source .venv/bin/activate
python -m pip install -r requirements.txt
npm ci
npm run tauri dev
```

To use an existing unpacked Phonon-2.5 model directory, set `VTYPE_MODEL_PATH`
before launching VType.
