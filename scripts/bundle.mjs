import { spawnSync } from "node:child_process";
import { constants } from "node:fs";
import { access, copyFile, mkdir } from "node:fs/promises";
import { delimiter, resolve } from "node:path";

const targets = { linux: "deb,rpm", win32: "nsis", darwin: "dmg" }[process.platform];
if (!targets) throw new Error(`Installer builds are unavailable on ${process.platform}.`);

const executable = process.platform === "win32" ? "uv.exe" : "uv";
let uv;
for (const directory of (process.env.PATH ?? "").split(delimiter).filter(Boolean)) {
  const candidate = resolve(directory, executable);
  try {
    await access(candidate, constants.X_OK);
    uv = candidate;
    break;
  } catch {
    // Try the next PATH entry.
  }
}
if (!uv) throw new Error("The build machine needs uv on PATH. Installer users do not need it.");

await mkdir("src-tauri/.bundle-runtime", { recursive: true });
await copyFile(uv, `src-tauri/.bundle-runtime/${executable}`);
const config = JSON.stringify({
  bundle: { resources: { [`.bundle-runtime/${executable}`]: `runtime/${executable}` } },
});
const build = spawnSync(process.execPath, [
  "node_modules/@tauri-apps/cli/tauri.js", "build",
  "--bundles", targets, "--config", config, ...process.argv.slice(2),
], { stdio: "inherit" });
if (build.error) throw build.error;
process.exitCode = build.status ?? 1;
