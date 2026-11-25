# Contributing to AztroMidi

Thanks for wanting to help! Bug reports, ideas and pull requests are all welcome.

## Getting started

You need **Windows 10/11 x64**, [Rust stable](https://rustup.rs/) with the MSVC toolchain,
and Visual Studio Build Tools with the C++ desktop workload.

```powershell
git clone https://github.com/Ayzmi/AztroMidi.git
cd AztroMidi
cargo run --locked
cargo test --locked
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
```

Use **Preview on this PC** and set test hotkeys to **None** during local UI testing.

## Project structure

| Path | What lives there |
|---|---|
| `src/midi.rs`, `src/schedule.rs`, `src/humanize.rs`, `src/keys.rs` | MIDI parsing, note scheduling, humanization and key mapping |
| `src/playback.rs` | Playback thread, transport and output routing |
| `src/desktop.rs`, `src/desktop/ui.rs` | Main desktop window and visualizer |
| `src/library.rs`, `src/settings.rs` | Song management and portable preferences |
| `src/win.rs`, `src/win/mini.rs` | Windows input, preview synth, single instance, global hotkeys and mini player |
| `assets/` | App icon and Windows manifest |
| `tools/` | Helper scripts (e.g. icon generator) |
| `docs/` | Images and documentation |

## Guidelines

- **Keep it light.** Prefer the standard library and existing dependencies.
- **Engine changes need tests.** Add coverage in the relevant Rust module.
- **Format with rustfmt.** Keep comments short and explain the reasoning.
- **Reuse the palette.** The desktop and mini player share the colors in `src/appearance.rs`.
- **No game exploits.** AztroMidi only sends normal key presses. Changes that inject into
  Roblox, read its memory or bypass its anti-cheat will not be accepted.

## Pull requests

1. Fork the repo and create a branch: `git checkout -b feature/my-idea`
2. Make your change and run the tests.
3. Add a line under **Unreleased** in `CHANGELOG.md`.
4. Open a pull request and fill in the template.

CI builds the app and runs the tests on every pull request.

## Releasing (maintainers)

1. Move the **Unreleased** notes in `CHANGELOG.md` under a new version heading, e.g. `## [1.4.0] - 2026-11-15`.
2. Update the package version in `Cargo.toml` and refresh `Cargo.lock`.
3. Commit, then tag and push:
   ```powershell
   git tag v1.4.0
   git push origin main --tags
   ```
4. The **Release** workflow builds the executable and zip and publishes the GitHub Release
   with the notes from `CHANGELOG.md`.
