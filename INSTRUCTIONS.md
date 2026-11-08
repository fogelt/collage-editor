### System Instructions for LLM Builder

**Role:** Expert Rust Developer specializing in GUI applications and image processing.

**Project Goal:**
Create a high-performance, lightweight Rust desktop application for importing images and arranging them into a customizable collage with grid snapping, free-form cursor movement, and final image export.

**Core Requirements:**
1. **Window Management:**
   - Use `winit` or `tauri` to detect the primary monitor resolution on launch.
   - Set the initial window size to a sensible, user-friendly proportion of the screen (e.g., 60% of the screen area).
   - Ensure the window is draggable and responsive.

2. **Core Functionality:**
   - **Image Import & Drag-and-Drop:** Implement a file-drop zone and import mechanism that accepts multiple image files.
   - **Collage Canvas:** Provide an interactive canvas workspace to arrange imported images.
   - **Grid Support:** Implement optional grid snapping to make aligning and organizing images fast and clean.
   - **Free-Form Movement:** Allow users to grab any image with the cursor and move it freely across the canvas.
   - **Output:**
     - Render and export the completed collage as a single high-resolution image file.
     - Prompt the user to select the save location and output format.
     - Also provide a simple one-click option to put all images into a self-contained HTML file.

3. **Technical Stack Preferences:**
   - **Language:** Rust.
   - **GUI Framework:** Tauri (preferred for web-tech UI) or iced (for a pure Rust approach).
   - **Image Library:** `image` and `imageproc`.
   - **Concurrency:** Utilize `rayon` or `tokio` to handle heavy image rendering and composition tasks in the background to ensure the UI remains responsive.

4. **Design Philosophy:**
   - **Simplicity:** Keep the UI minimal—focus purely on the collage workspace, canvas controls, and export actions.
   - **Performance:** Ensure low memory overhead and smooth 60fps cursor dragging and snapping.
   - **Reliability:** Include basic error handling for unsupported file types, canvas boundary overflows, or write-permission issues.
   - **Clean chrome:** No technical status chips or debug information in the header. The canvas and primary actions stay the focus.

**Deliverables:**
- A `Cargo.toml` with necessary dependencies.
- A concise `main.rs` (and necessary modules) that handles the window lifecycle, canvas interaction, and the image composition and export pipeline.

### Shared Foundation (match `photo-resizer`)

**Language & settings:**
- Provide an English flag and a Swedish flag in the top bar to switch the entire UI language.
- Localize all UI text, buttons, tooltips, validation messages, and processing errors.
- Persist the chosen language in a user config file:
  - Windows: `%APPDATA%\collage-editor\settings.conf`
  - macOS: `~/Library/Application Support/collage-editor/settings.conf`
  - Linux: `$XDG_CONFIG_HOME/collage-editor/settings.conf` or `~/.config/collage-editor/settings.conf`
- Default to English when no config exists. Do not reset the user's choice on upgrade.
- Reuse the same language/config approach and flag assets as `photo-resizer`.

**Final Build & Packaging:**
- Provide a GitHub Actions workflow (`.github/workflows/release.yml`) that runs on `v*` tags.
- Build and test on Linux, macOS (Intel and Apple Silicon), and Windows.
- Publish a GitHub Release containing native installers, not zip archives:
  - Windows `.exe` (NSIS)
  - macOS `.dmg`
  - Linux `.deb` and `.AppImage`
- Use `cargo-packager` with `[package.metadata.packager]` in `Cargo.toml`, matching the
  `photo-resizer` release setup.

**App icon:**
- Provide a Python script (`tools/make_icon.py`) that draws a fitting collage app icon
  from scratch and exports PNG sizes plus a Windows `.ico`.
- Commit the generated icons under `assets/icons/`.
- Use the icon for the window at runtime and embed it in the Windows executable.

**Icon package / UI polish:**
- Use an icon package (Font Awesome via `iced_font_awesome`) instead of plain text
  where an icon is clearer.
- Prefer compact icon-only controls with tooltips for secondary actions
  (toggle grid, clear canvas, export collage).
- Replace text status badges with color-coded status icons so the workspace stays
  clean and scannable.
- Keep the loading indicator lightweight and background-free — no canvas-based
  loader that introduces visible boxes behind the spinner.
