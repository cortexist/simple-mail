// Runs the Tauri CLI with NO_STRIP=true on every platform. linuxdeploy's
// bundled strip cannot read newer ELF sections and fails AppImage builds;
// the inline `NO_STRIP=true tauri` form only works in POSIX shells, so it
// broke the Windows release build. A value already in the environment wins.
import { spawnSync } from "node:child_process";

const result = spawnSync("tauri", process.argv.slice(2), {
  stdio: "inherit",
  env: { NO_STRIP: "true", ...process.env },
  shell: process.platform === "win32",
});
if (result.error) {
  console.error(result.error);
  process.exit(1);
}
process.exit(result.status ?? 1);
