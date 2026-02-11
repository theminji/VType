# VType
## WORK IN PROGRESS

Voice typing without distractions

Default hotkey is `Ctrl/Cmd + Alt + r`. This will start recording (should show an icon) and then press it again to stop recording and transcribe

Only tested on Linux and Windows for now...

### UPDATE

Linux and Windows binaries in [releases](https://github.com/theminji/VType/releases/tag/releases)

The first time you run the app with the hotkey, it will take a little bit to download the [model](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx)

```
pip install -r requirements.txt
npm install
npm run tauri dev
```
