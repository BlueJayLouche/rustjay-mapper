# RustJay Mapper

A high-performance projection mapping application in Rust with GPU-accelerated rendering, NDI and Syphon I/O, and AprilTag-based video wall calibration.

## Features

- **Multiple Input Types**
  - **Webcam** — direct camera capture via nokhwa
  - **NDI** — Network Device Interface sources (including OBS via NDI plugin)
  - **Syphon** — macOS GPU texture sharing (Resolume, TouchDesigner, etc.)
- **Independent Dual Inputs** — each slot can use a different source type; hot-swappable without restart
- **GPU-Accelerated Rendering** — wgpu pipeline with configurable internal resolution
- **Dual Window Architecture**
  - Fullscreen output window with hidden cursor for clean projection
  - Separate ImGui control window
- **Projection Mapping** — corner pinning, bilinear interpolation, per-input scale/offset/rotation
- **Blend Modes** — Normal (mix), Add, Multiply, Screen
- **Video Matrix / Grid Mapping**
  - Subdivide a single HDMI output into an N x M grid for multi-display walls
  - Per-cell aspect ratio, orientation, and output position mapping
  - AprilTag auto-detection of screen layout from a photo or live input
  - Named preset save/load
- **AprilTag Calibration**
  - Runtime marker generation via the apriltag C library (all 587 tag36h11 markers available — no pre-generated files required)
  - Pattern display adapts to the user's configured grid size
  - Pure Rust detection with aspect ratio and orientation inference
- **Output**
  - NDI output with dedicated sender thread
  - Syphon output (macOS) for zero-copy GPU sharing
- **Audio Reactive** — optional cpal audio input with 8-band FFT

## Architecture

See [DESIGN.md](DESIGN.md) for the full architecture document.

### Design Documents

| Document | Description |
|----------|-------------|
| [DESIGN.md](DESIGN.md) | Architecture, modules, threading, data flow |
| [DESIGN_GUI_LAYOUT.md](DESIGN_GUI_LAYOUT.md) | Visual GUI layout specification |
| [DESIGN_LOCAL_OUTPUT.md](DESIGN_LOCAL_OUTPUT.md) | Local video output design (Syphon/Spout/v4l2loopback) |
| [DESIGN_LOCAL_INPUT.md](DESIGN_LOCAL_INPUT.md) | Local video input design (Syphon/Spout/v4l2loopback) |

```
┌─────────────────┐      ┌─────────────────────┐
│  Control Window │      │   Output Window      │
│   (ImGui UI)    │      │  (Fullscreen, No     │
│                 │      │   Cursor)            │
└────────┬────────┘      └──────────┬───────────┘
         │                          │
         └──────────┬───────────────┘
                    │
           ┌────────▼────────┐
           │   wgpu Engine   │
           │  (GPU Render)   │
           └────────┬────────┘
                    │
        ┌───────────┼───────────┐
        │           │           │
┌───────▼──────┐ ┌──▼───────┐ ┌▼──────────────┐
│  NDI Input   │ │ Syphon   │ │  NDI / Syphon  │
│   Thread     │ │ Input    │ │  Output        │
└──────────────┘ └──────────┘ └────────────────┘
```

## Install

Pre-built binaries are available on the [Releases](https://github.com/BlueJayLouche/rustjay-mapper/releases) page.

| Platform | Format | Notes |
|----------|--------|-------|
| macOS Apple Silicon | `.dmg` | Ad-hoc signed. Right-click → Open on first launch. |
| macOS Intel | `.dmg` | Ad-hoc signed. Right-click → Open on first launch. |
| Linux x86_64 | `.tar.gz` | Requires Vulkan-capable GPU. |

Download the `.dmg`, open it, and drag RustJay Mapper to your Applications folder. On Linux, extract the tarball and run the binary directly.

> NDI is not included in release builds. For NDI support, install the NDI SDK and build from source with `cargo build --release`.

## Building from Source

### Requirements

- Rust 1.70+
- macOS (primary platform; Linux/Windows possible with reduced feature set)
- NDI SDK — [download from ndi.video](https://ndi.video/tools-sdk/)
- Syphon framework (macOS) — provided by `syphon-rs` sibling repo

### Standard Build

```bash
cargo build --release
```

### Build Without Webcam Support

```bash
cargo build --release --no-default-features
```

### Run

```bash
cargo run --release
```

### Syphon Setup (macOS)

Syphon is enabled automatically on macOS. The build system finds the framework at `../syphon-rs/syphon-lib/Syphon.framework`.

The `syphon-rs` repo must be present as a sibling directory:

```
developer/rust/
├── syphon-rs/          ← must exist
│   └── syphon-lib/
│       └── Syphon.framework
└── rustjay-mapper/
```

If your layout differs, override the path:

```bash
SYPHON_FRAMEWORK_DIR=/path/to/syphon-rs/syphon-lib cargo build --release
```

### NDI Setup

The build system probes standard macOS SDK install paths (e.g. `/Library/NDI SDK for Apple/lib/macOS`). Override with:

```bash
NDI_SDK_DIR=/path/to/ndi/sdk cargo build --release
```

## Usage

1. **Start the application** — two windows appear: the output window and the control window.

2. **Inputs tab** — select source type (Webcam, NDI, Syphon) for Input 1 and Input 2. Click Refresh to detect new sources.

3. **Mapping tab** — adjust corner pinning, scale, offset, rotation, blend mode, and opacity per input.

4. **Matrix tab** — configure an N x M grid for video wall output:
   - Set grid dimensions (e.g. 3x3, 4x4)
   - Click **Show AprilTag Pattern** to display calibration markers on all grid cells
   - Use **Load from Photo** or **Auto-Detect from Current Input** to automatically detect screen positions and aspect ratios
   - Fine-tune cell-to-output mappings manually if needed

5. **Output tab** — start NDI or Syphon output, toggle fullscreen.

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Shift+F` | Toggle fullscreen (output window) |
| `Escape` | Exit |

### OBS Integration

1. Install the OBS NDI plugin
2. In OBS: Tools > NDI Output Settings > Enable
3. In RustJay Mapper: select the OBS NDI source in the Inputs tab

## Configuration

Edit `config.toml`:

```toml
[output_window]
width = 1280
height = 720
fullscreen = false
vsync = true
fps = 60

[resolution]
internal_width = 1920
internal_height = 1080
```

## Project Structure

```
src/
├── main.rs                # Entry point
├── lib.rs                 # Library crate root
├── config.rs              # TOML configuration loading
├── app/
│   ├── mod.rs             # App struct, run_app(), toggle_fullscreen()
│   ├── commands.rs        # Input/output command dispatch
│   ├── update.rs          # Per-frame updates, matrix pattern sync
│   └── events.rs          # winit ApplicationHandler impl
├── core/
│   ├── state.rs           # SharedState (thread-safe shared state)
│   └── vertex.rs          # GPU vertex types
├── engine/
│   ├── renderer.rs        # wgpu render pipeline, blit, AprilTag pattern display
│   └── texture.rs         # Texture utilities
├── gui/
│   ├── gui.rs             # ImGui control interface
│   └── renderer.rs        # ImGui wgpu backend
├── input/
│   ├── mod.rs             # InputManager, InputSource
│   ├── ndi.rs             # NDI receiver
│   ├── syphon_input.rs    # Syphon input (macOS)
│   └── webcam.rs          # Webcam capture (optional)
├── ndi/
│   └── output.rs          # NDI sender thread
├── output/
│   ├── mod.rs             # OutputManager (NDI + Syphon)
│   ├── syphon.rs          # Syphon output (macOS)
│   └── readback.rs        # GPU readback for CPU-based outputs
├── audio/
│   └── input.rs           # cpal capture + 8-band FFT
└── videowall/
    ├── apriltag.rs         # AprilTag detector + runtime marker generator
    ├── apriltag_auto_detect.rs  # Auto-detection of screen layout
    ├── aruco.rs            # Legacy ArUco dictionary (fallback)
    ├── calibration.rs      # Calibration state machine
    ├── grid_mapping.rs     # Grid subdivision + cell mapping
    ├── matrix_renderer.rs  # Video matrix GPU renderer
    ├── renderer.rs         # Video wall GPU renderer
    ├── quad_mapper.rs      # Quad warping utilities
    ├── config.rs           # Video wall serialisation
    └── test_pattern.rs     # Test pattern generator
```

## Troubleshooting

### Syphon Framework Not Found

If you see `dyld: Library not loaded: Syphon.framework`:

1. Verify the framework exists: `ls ../syphon-rs/syphon-lib/Syphon.framework`
2. Override the path: `SYPHON_FRAMEWORK_DIR=/path/to/dir cargo build --release`

### NDI SDK Not Found

Install the NDI SDK from [ndi.video](https://ndi.video/tools-sdk/), then:

```bash
export NDI_SDK_DIR="/path/to/NDI/sdk"
cargo build --release
```

### AprilTag Pattern Not Displaying

Markers are generated at runtime via the apriltag C library — no PNG files on disk are required. If the pattern still doesn't appear, check the log output for errors (run with `RUST_LOG=info`).

## License

MIT
