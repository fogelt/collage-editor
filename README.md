# Collage Editor

A lightweight desktop app for arranging photos into a collage and exporting the
result. Built with Rust, [`iced`](https://iced.rs), and the `image` crate.

## Features

- Drag and drop many photos at once, or pick them from the file dialog.
- Five starting layouts: grid, mosaic, strip, hero, and stack.
- Free-form dragging on the canvas with optional grid snapping.
- Export the collage as a high-resolution PNG or JPEG.
- One-click export to a self-contained HTML file (images embedded as data URLs).
- English and Svenska, remembered between launches.

## Controls

| Control | Action |
| --- | --- |
| **Add photos** | Choose images from disk. Folders can also be dropped on the window. |
| Photo tiles | Drag to move; drop near an edge, centre, another photo, or a grid line to snap. |
| Pattern icons | Switch between grid, mosaic, strip, hero, and stack layouts. |
| Magnet | Toggle grid and edge snapping. |
| Trash | Clear the canvas. |
| Download | Export the collage as an image. |
| File icon | Export the collage as a standalone HTML page. |
| Flags | Switch the whole interface between English and Swedish. |

## Build and run

```sh
cargo run --release
```

Release builds are recommended: image decoding and the software renderer are
optimized, and the app starts with a small memory footprint. To run the test
suite:

```sh
cargo test
```

## Settings

The language choice is stored in a small config file:

- **Windows:** `%APPDATA%\collage-editor\settings.conf`
- **macOS:** `~/Library/Application Support/collage-editor/settings.conf`
- **Linux:** `$XDG_CONFIG_HOME/collage-editor/settings.conf` (or
  `~/.config/collage-editor/settings.conf`)

English is used when no config file exists.

## Install

Releases include native installers:

- **Windows** — `.exe` (NSIS)
- **macOS** — `.dmg` (Intel and Apple Silicon)
- **Linux** — `.deb` or `.AppImage`

## Project layout

- `src/main.rs` — application state, UI, and window setup.
- `src/canvas.rs` — interactive collage canvas and hit testing.
- `src/collage.rs` — image loading, layout patterns, snapping, and export.
- `src/i18n.rs` — translations and the persisted language preference.
- `tools/` — scripts that generate the app icon and flag images.
