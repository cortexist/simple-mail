// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Every process this binary starts runs on this allocator. See the note on the
// `mimalloc` dependency in `Cargo.toml` for why the platform default is not used.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    mail_lib::run()
}
