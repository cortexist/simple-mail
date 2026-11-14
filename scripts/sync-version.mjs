import { readFileSync, writeFileSync } from "node:fs";

const version = JSON.parse(readFileSync("package.json", "utf8")).version;

const cargoToml = "src-tauri/Cargo.toml";
const tomlNext = readFileSync(cargoToml, "utf8").replace(/^version = ".*"/m, `version = "${version}"`);
writeFileSync(cargoToml, tomlNext);

// Bump the simple-mail entry in Cargo.lock so the next `cargo build` doesn't
// re-touch it after the release commit.
const cargoLock = "src-tauri/Cargo.lock";
const lock = readFileSync(cargoLock, "utf8");
const lockPattern = /(\[\[package\]\]\nname = "simple-mail"\nversion = ").*?(")/;
if (!lockPattern.test(lock)) {
  throw new Error(`Could not find simple-mail package entry in ${cargoLock}`);
}
const lockNext = lock.replace(lockPattern, `$1${version}$2`);
writeFileSync(cargoLock, lockNext);
