import { readFileSync, writeFileSync } from "node:fs";

const version = JSON.parse(readFileSync("package.json", "utf8")).version;
const cargoToml = "src-tauri/Cargo.toml";
const next = readFileSync(cargoToml, "utf8").replace(/^version = ".*"/m, `version = "${version}"`);
writeFileSync(cargoToml, next);
