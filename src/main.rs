#![cfg_attr(
    all(windows, not(debug_assertions), not(test)),
    windows_subsystem = "windows"
)]

mod humanize;
mod keys;
mod midi;
mod schedule;
mod settings;

#[cfg(windows)]
mod appearance;
#[cfg(windows)]
mod desktop;
#[cfg(windows)]
mod library;
#[cfg(windows)]
mod playback;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    desktop::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("AztroMidi requires Windows.");
}
