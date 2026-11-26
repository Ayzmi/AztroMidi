# Changelog

All notable changes to AztroMidi are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.5.0] - 2026-11-26

### Added
- Voice Chat Piano output for Windows audio devices, with automatic VB-CABLE routing, an optional microphone mix, and separate piano, microphone and room reverb controls.
- A sampled acoustic grand piano for Voice Chat Piano, with three velocity layers and MIDI sustain.
- Local piano monitoring through a second playback device, with its own volume control.
- Play the sampled piano live with the computer keyboard in Voice Chat Piano mode.
- Keep the selected microphone live between songs and check its input level in the Mixer.

### Changed
- Group desktop settings into Audio, Library & window, and Hotkeys tabs. Align controls and show long device names on hover.
- Refine the player layout, with a clearer timeline and transport, a wider piano area at small window sizes, and separate scroll positions for each page.
- Reuse library search results between frames and limit how often the mini player refreshes.
- Prepare Voice Chat Piano devices ahead of playback and let the piano and room reverb decay on Pause and Stop.

### Fixed
- Save settings off the UI thread and wait for the window to settle before saving its placement.
- Capture microphones in their Windows mix format and preserve speech on an active input channel. Show the source level in the mic meter before it reaches the voice mix.

## [1.4.0] - 2026-11-26

### Added
- Drag & drop MIDI files or folders onto the window to move them into your library folder
  (or copy them, or only add them to the list under Settings → Library → Dropped files).
- Open `.mid` files from Explorer with AztroMidi; if it is already running, the files go to that window.
- Right-click menu on songs: play, favorite, show in folder, copy path, remove from list.
- Song lengths in the library.
- Keyboard shortcuts: Space, arrow keys, +/−, Enter, Delete, Ctrl+F, Ctrl+O, Ctrl+1/2/3, Ctrl+M.
- The playing song is shown in the title bar and the taskbar.
- Window size, position and the last page are remembered.
- Releases include `SHA256SUMS.txt` for verifying downloads.

### Changed
- Local builds now go to `dist/latest` by default and accept absolute output paths.
- Port the desktop app and MIDI engine to Rust. The Windows x64 executable runs without a separate runtime.
- Keep the existing portable settings, favorites and song library. Keyboard playback waits for the Roblox window to gain focus.
- Improve contrast across the desktop UI and mini player, with clearer transport controls and aligned settings.
- New desktop look: sidebar navigation, slimmer title bar with standard window buttons,
  Windows' own font, flatter panels and smaller corners.
- Buttons and icons have accessible names for screen readers.

### Fixed
- Keep the next note due when Roblox loses focus during playback and restore the sustain pedal on resume or seek.
- Report Windows output errors with a retryable playback state.
- Refresh song lengths after file changes, flag unreadable MIDI files, and let the import queue recover after interruptions.
- Keep drum-only tracks visibly skipped without losing track choices when the setting changes.
- Let the mini player take over the topmost position while it is open.
- Retry settings saves after temporary write failures and replace builds only after the new executable is copied.
- Opening an unreadable song no longer starts the previously loaded song by mistake.
- Validate saved playback and humanize values before creating controls or scheduling notes.
- Recover null settings sections and library strings while keeping other saved preferences.
- Write settings to a temporary file before replacement to avoid truncating the previous file during a failed write.
- Use unique write-access probes instead of overwriting an existing `.write_test` file.
- Reject truncated MIDI events and invalid chunk lengths without reading into another track.
- Reject invalid time divisions and asynchronous format 2 files instead of playing incorrect timing.
- Correct SMPTE 29.97 fps timing for MIDI files using drop-frame time division.
- Clicking another song while one is playing no longer stops the music; it only selects it.

## [1.3.1] - 2026-10-29

### Changed
- The title bar now behaves like a normal Windows title bar: drag a maximized window down to restore it,
  double-click to maximize, snap to screen edges, and maximizing no longer covers the taskbar.

### Fixed
- The Pause button in AztroMidi did not pause in Roblox mode; it jumped back to Roblox instead.
  Only the F1 hotkey jumps back now, buttons always pause.
- The mini player could shrink to 2 px wide and seem to disappear after F7, especially after it had been dragged.
- Hotkeys (F1, F7...) stopped working when AztroMidi was opened twice. Starting it again now
  brings the running window back instead of opening a second copy.

## [1.3.0] - 2026-10-29

### Added
- Pressing Play in AztroMidi switches to the Roblox window automatically.
- When playback is waiting because another app is focused, F1 or Play jumps back to Roblox and continues.
- Album tile in Now Playing, song icons in the library (the current song is highlighted), empty state for tracks.
- Test project, CI and automatic GitHub Releases.

### Changed
- More compact Quick controls.

### Removed
- Background mode. Roblox ignores keyboard input sent to a window that is not active, so it could not work reliably.

## [1.2.0] - 2026-10-29

### Added
- Experimental background mode test button (removed again in 1.3.0).

### Fixed
- The last song is remembered immediately, even if the app is closed unexpectedly.

## [1.1.0] - 2026-10-29

### Added
- Mini player that floats in the top-right corner of the Roblox window and never steals focus.
- Preview mode to listen to a song on your PC.
- Upcoming keys in virtual piano sheet style, e.g. `t [tuo] y`.
- Favorites and reopening the last song on start.
- Loop and auto-next toggles in the transport bar.

### Fixed
- Window now fits 1366x768 screens.

## [1.0.0] - 2026-10-29

### Added
- First release: Roblox 61/88-key layouts, humanized playback, tracks with solo, speed, transpose,
  sustain pedal, velocity keys, global hotkeys and a song library.
