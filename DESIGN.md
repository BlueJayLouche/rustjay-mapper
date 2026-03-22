# RustJay Mapper - Design Document

## Overview

A high-performance Rust video application for projection mapping with NDI and Syphon I/O, dual-window architecture (preview/control + fullscreen output), and GPU-accelerated rendering via wgpu.

## Goals

- **High Performance**: 60fps+ rendering with minimal latency
- **Dual Window Architecture**: Control window for UI, fullscreen output window with hidden cursor
- **NDI + Syphon Integration**: Both input (receive) and output (send) with dedicated threads
- **Cross-Platform**: macOS primary, with Linux/Windows support potential
- **Projection Mapping Ready**: Fullscreen output, cursor hiding, configurable resolutions

---

## Architecture

### High-Level Structure

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         RUSTJAY MAPPER                                       │
│                                                                              │
│  ┌─────────────────────┐      ┌─────────────────────────────────────────┐   │
│  │   CONTROL WINDOW    │      │          OUTPUT WINDOW                   │   │
│  │  (imgui + preview)  │      │      (Fullscreen, No Cursor)             │   │
│  │                     │      │                                          │   │
│  │  ┌───────────────┐  │      │  ┌────────────────────────────────────┐  │   │
│  │  │ Source        │  │      │  │       RENDER PIPELINE               │  │   │
│  │  │ Selector      │  │      │  │  ┌──────────┐ ┌──────────┐         │  │   │
│  │  ├───────────────┤  │      │  │  │  Input   │ │ Effects  │         │  │   │
│  │  │ Preview       │  │◄─────┼──┼──┤ Processor│ │  Stage   │         │  │   │
│  │  │ (320x180)     │  │      │  │  └────┬─────┘ └────┬─────┘         │  │   │
│  │  ├───────────────┤  │      │  │       └────────────┘                │  │   │
│  │  │ Output Ctrl   │  │      │  │            ↓                        │  │   │
│  │  │ (NDI/Syphon)  │  │      │  │  ┌──────────────────────┐           │  │   │
│  │  └───────────────┘  │      │  │  │    Output Mixer      │           │  │   │
│  │                     │      │  │  │  (Projection Mapped) │           │  │   │
│  │  ┌───────────────┐  │      │  │  └──────────┬───────────┘           │  │   │
│  │  │ Parameters    │  │      │  │             ↓                       │  │   │
│  │  │ (Real-time)   │◄─┼──────┼──┼─────────────┘                       │  │   │
│  │  └───────────────┘  │      │  └────────────────────────────────────┘  │   │
│  └─────────────────────┘      └─────────────────────────────────────────┘   │
│           ▲                              │                                  │
│           │         ┌────────────────────┴────────────────┐                 │
│           │         │           SHARED STATE               │                 │
│           │         │  (Parameters, Audio, Sources)        │                 │
│           │         └───────────────────────────────────────┘                 │
│           │                                                                   │
│  ┌────────┴─────────────────────────┐     ┌──────────────────────────────┐  │
│  │   NDI / SYPHON INPUT THREADS    │     │  NDI / SYPHON OUTPUT THREADS │  │
│  │  ┌────────────┐  ┌────────────┐ │     │  ┌────────────┐ ┌──────────┐ │  │
│  │  │ NDI Recv   │  │ Syphon In  │ │     │  │ NDI Send   │ │ Syphon   │ │  │
│  │  │ (BGRA→RGBA)│  │ (GPU zero  │ │     │  │ (RGBA→BGRA)│ │ Out      │ │  │
│  │  │            │  │  copy)     │ │     │  │            │ │ (GPU)    │ │  │
│  │  └────────────┘  └────────────┘ │     │  └────────────┘ └──────────┘ │  │
│  └──────────────────────────────────┘     └──────────────────────────────┘  │
│                                                                              │
│  ┌──────────────────────────────────────────────┐                           │
│  │              AUDIO INPUT                       │                           │
│  │  ┌────────────┐  ┌─────────────┐  ┌─────────┐ │                           │
│  │  │ cpal Input │──│ FFT (8-band)│──│ Shared  │─┘                           │
│  │  └────────────┘  └─────────────┘  │  State  │                              │
│  └───────────────────────────────────┴─────────┘                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Modules

### 1. Core Module (`src/core/`)
- **SharedState**: Thread-safe state shared between windows and threads
- **Vertex**: GPU vertex definitions for quad rendering

### 2. Application (`src/app/`)
Dual-window application handler implementing `winit::application::ApplicationHandler`:
- **`mod.rs`**: `App` struct definition, `run_app()` entry point, `toggle_fullscreen()`
- **`commands.rs`**: `dispatch_commands()` — processes `InputCommand` and `NdiOutputCommand` each frame; `apply_input_command(slot, cmd)` processes both slots with a single code path
- **`update.rs`**: `update_inputs()`, `process_matrix_test_pattern()`, `sync_video_wall_state()`, `sync_video_matrix_state()`
- **`events.rs`**: `ApplicationHandler` impl — `resumed()`, `window_event()`, `about_to_wait()`

Window roles:
- **Output Window**: Fullscreen-capable, cursor hidden, wgpu surface
- **Control Window**: ImGui-based UI, resizable, decorated

### 3. Input (`src/input/`)
- **InputManager**: Manages Input 1 and Input 2 with independent source types
- **NDI** (`ndi.rs`): Background thread receiver with BGRA→RGBA conversion, bounded channel with latest-frame-only semantics
- **Syphon** (`syphon_input.rs`): macOS GPU texture sharing (zero-copy via IOSurface)
- **Webcam** (`webcam.rs`): Optional, via nokhwa

### 4. Output (`src/output/` + `src/ndi/`)
- **OutputManager**: Coordinates NDI and Syphon outputs
- **NdiOutputSender** (`ndi/output.rs`): Dedicated background thread, bounded channel (capacity=2), `Box::leak` pattern
- **SyphonOutput** (`output/syphon.rs`): macOS GPU texture sharing
- **Readback** (`output/readback.rs`): GPU→CPU readback for CPU-based output paths

### 5. Rendering Engine (`src/engine/`)
GPU-accelerated pipeline using wgpu:
- **WgpuEngine** (`renderer.rs`): Main render pipeline, video matrix/wall rendering, AprilTag pattern display, blit-to-surface
- **Texture** (`texture.rs`): Texture creation and management utilities

### 6. GUI (`src/gui/`)
ImGui-based control interface with tabs: Inputs, Mapping, Matrix, Output, Settings.

### 7. Video Wall / Matrix (`src/videowall/`)
- **AprilTag detection** (`apriltag.rs`): Pure Rust detector + runtime marker generator via `apriltag-sys` FFI (supports all 587 tag36h11 markers without pre-generated files)
- **Auto-detection** (`apriltag_auto_detect.rs`): Infers screen positions, aspect ratios (4:3, 16:9, 21:9), and orientations from AprilTag distortion
- **Grid mapping** (`grid_mapping.rs`): N x M grid subdivision with per-cell output mapping
- **Matrix renderer** (`matrix_renderer.rs`): GPU renderer for grid-based multi-display output
- **Video wall renderer** (`renderer.rs`): GPU renderer for quad-mapped displays
- **Calibration** (`calibration.rs`): State machine calibration controller
- **Test patterns** (`test_pattern.rs`): Colour bars, grids, numbered patterns, checkerboard, gradient

### 8. Audio (`src/audio/`)
Optional audio analysis:
- **AudioInput**: cpal-based audio capture
- **FFT**: 8-band frequency analysis

---

## Thread Architecture

```
MAIN THREAD (Event Loop)
├── Polls window events
├── Updates shared state
├── Submits GPU commands
└── Requests redraws

NDI INPUT THREAD (Per Source)
├── Finds NDI source
├── Receives video frames
├── Converts BGRA → RGBA
└── Sends to bounded queue

NDI OUTPUT THREAD (Singleton)
├── Receives frames from queue
├── Converts RGBA → BGRA/BGRX
├── Sends via NDI SDK
└── Logs stats periodically (30s)

AUDIO THREAD (cpal callback)
├── Captures audio samples
├── Performs FFT analysis
└── Updates shared audio state
```

---

## Data Flow

### Input Flow
```
Webcam:  Camera → MJPEG/YUYV → RGBA → Frame Queue → GPU Upload → Shader
NDI:     Network → NDI SDK → Receiver Thread → BGRA→RGBA → Frame Queue → GPU Upload → Shader
Syphon:  IOSurface → GPU zero-copy texture → Shader
```

### Output Flow
```
Shader → GPU Render Target
  ├── Blit to surface (output window)
  ├── Syphon: GPU texture → IOSurface → receiving apps (zero-copy)
  └── NDI: GPU Readback → RGBA→BGRA → Frame Queue → Sender Thread → NDI SDK → Network
```

---

## Key Design Decisions

### 1. Dedicated Output Threads
NDI SDK send operations can block; moving them off the main thread prevents frame drops. The thread owns the NDI `Sender`, receives frames via a bounded channel, and persists via `Box::leak`.

### 2. Multi-Input Support
- **Input Types**: Webcam, NDI, OBS (via NDI), Syphon (macOS GPU zero-copy)
- **Independent Mapping**: Each input can be selected independently
- **Hot Swappable**: Change inputs on the fly without restart
- **Command dispatch**: GUI writes `InputCommand` variants into SharedState; `App::apply_input_command(slot, cmd)` processes both slots with a single code path

### 3. Bounded Frame Queues
- **Input Queue**: Capacity 5, drops oldest when full (latest-frame semantics)
- **Output Queue**: Capacity 2, drops when full (low-latency over reliability)
- Prevents memory growth under load; prioritises fresh frames

### 4. Dual Window with Shared GPU Context
- Single `wgpu::Instance`, shared `Device` and `Queue` (wrapped in `Arc`)
- Output window owns the wgpu surface; control window shares GPU resources
- ImGui renderer uses the same device/queue for UI rendering

### 5. Runtime AprilTag Generation
Marker images are generated at runtime via `apriltag_sys::apriltag_to_image()` FFI rather than loaded from disk. This eliminates file-path dependencies and supports any grid size up to 9x9 (81 markers) without needing pre-generated PNGs.

---

## File Structure

```
rustjay-mapper/
├── Cargo.toml
├── build.rs                 # macOS rpath setup for Syphon + NDI
├── config.toml              # Runtime configuration
├── README.md
├── DESIGN.md                # This document
├── AGENTS.md                # Guide for AI agents working on this codebase
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # Library crate root
│   ├── config.rs            # TOML configuration loading
│   ├── app/
│   │   ├── mod.rs           # App struct, run_app(), toggle_fullscreen()
│   │   ├── commands.rs      # Input/output command dispatch
│   │   ├── update.rs        # Per-frame updates, matrix pattern sync
│   │   └── events.rs        # winit ApplicationHandler impl
│   ├── core/
│   │   ├── state.rs         # SharedState (thread-safe)
│   │   └── vertex.rs        # GPU vertex types
│   ├── engine/
│   │   ├── renderer.rs      # wgpu render pipeline + blit + AprilTag pattern
│   │   └── texture.rs       # Texture utilities
│   ├── gui/
│   │   ├── gui.rs           # ImGui control interface
│   │   └── renderer.rs      # ImGui wgpu backend
│   ├── input/
│   │   ├── mod.rs           # InputManager, InputSource
│   │   ├── ndi.rs           # NDI receiver thread
│   │   ├── syphon_input.rs  # Syphon input (macOS)
│   │   └── webcam.rs        # Webcam capture (optional)
│   ├── ndi/
│   │   └── output.rs        # NDI sender thread
│   ├── output/
│   │   ├── mod.rs           # OutputManager
│   │   ├── syphon.rs        # Syphon output (macOS)
│   │   └── readback.rs      # GPU readback for CPU-based outputs
│   ├── audio/
│   │   └── input.rs         # cpal capture + 8-band FFT
│   └── videowall/
│       ├── apriltag.rs              # AprilTag detector + runtime marker generator
│       ├── apriltag_auto_detect.rs  # Screen layout auto-detection
│       ├── aruco.rs                 # Legacy ArUco dictionary (fallback)
│       ├── calibration.rs           # Calibration state machine
│       ├── grid_mapping.rs          # Grid subdivision + cell mapping
│       ├── matrix_renderer.rs       # Video matrix GPU renderer
│       ├── renderer.rs              # Video wall GPU renderer
│       ├── quad_mapper.rs           # Quad warping utilities
│       ├── config.rs                # Video wall serialisation
│       └── test_pattern.rs          # Test pattern generator
└── assets/
    └── apriltags/           # Optional pre-generated marker PNGs (fallback)
```

---

## Build Configuration (`build.rs`)

On macOS, `build.rs` embeds runtime rpaths so both Syphon and NDI dylibs are found by `dyld`:

| Library | Discovery | Env var override |
|---------|-----------|-----------------|
| `Syphon.framework` | `<workspace>/../syphon-rs/syphon-lib/` or cargo git dep cache | `SYPHON_FRAMEWORK_DIR` |
| `libndi.dylib` | Probes standard SDK install paths (e.g. `/Library/NDI SDK for Apple/lib/macOS`) | `NDI_SDK_DIR` |

`@executable_path` and `@loader_path` entries are also added to support bundled app deployments.

---

## Dependencies

```toml
[dependencies]
winit = "0.30"
wgpu = { version = "25.0", features = ["spirv"] }
pollster = "0.3"
glam = { version = "0.29", features = ["bytemuck", "serde"] }
bytemuck = { version = "1.21", features = ["derive"] }
image = { version = "0.25", features = ["png", "jpeg"] }
grafton-ndi = "0.11"
crossbeam = "0.8"
cpal = "0.15"
rustfft = "6.2"
realfft = "3.4"
imgui = "0.12"
imgui-wgpu = "0.25"
imgui-winit-support = "0.13"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
log = "0.4"
env_logger = "0.11"
anyhow = "1.0"
thiserror = "2.0"
apriltag = "0.4"
apriltag-sys = "0.3"
rfd = "0.15"

# macOS only
syphon-core = { git = "..." }
syphon-wgpu = { git = "..." }
metal = "0.29"
```

---

## Video Matrix / Grid Mapping

The application supports projection mapping to multiple displays via an HDMI video matrix using a **grid subdivision approach**:

```
┌─────────────────────────────────────────────────────────────────┐
│                     INPUT TEXTURE                                │
│              (Subdivided into configurable N×M grid)            │
│                                                                  │
│   ┌───┬───┬───┐                                                  │
│   │ 0 │ 1 │ 2 │  Each cell can be mapped to output grid cell    │
│   ├───┼───┼───┤                                                  │
│   │ 3 │ 4 │ 5 │                                                  │
│   ├───┼───┼───┤                                                  │
│   │ 6 │ 7 │ 8 │                                                  │
│   └───┴───┴───┘                                                  │
└─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│              RENDER PIPELINE (per mapped cell)                   │
│                                                                  │
│   Input Cell → Aspect Ratio → Orientation → Output Position     │
│                    ↑                ↑                           │
│            (AprilTag detected)   (AprilTag detected)            │
└─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│              SINGLE FULLSCREEN OUTPUT                            │
│         (Sent to HDMI Video Matrix → Physical Displays)         │
│                                                                  │
│   ┌────┬────┬────┐                                               │
│   │ A  │ B  │ C  │  A=Cell 0 mapped (4:3, 0°)                   │
│   ├────┼────┼────┤  B=Cell 1 mapped (16:9, 0°)                  │
│   │ -- │ -- │ -- │  C=Cell 2 mapped (16:9, 90°)                 │
│   ├────┼────┼────┤  --=Unmapped (black)                         │
│   │ -- │ -- │ -- │                                               │
│   └────┴────┴────┘                                               │
└─────────────────────────────────────────────────────────────────┘
```

### AprilTag Calibration

The calibration workflow uses AprilTag fiducial markers (tag36h11 family, pure Rust detection via the `apriltag` crate):

1. **Pattern display**: The app generates a calibration pattern with one unique AprilTag per grid cell, sized to the user's configured grid (up to 9x9). Markers are generated at runtime via FFI — no pre-generated files needed.

2. **Detection**: A photo of the displays (or a live camera feed) is analysed. The detector identifies each marker's ID, position, and corner distortion.

3. **Inference**: From the tag distortion the system infers each display's aspect ratio (4:3, 16:9, or 21:9) and builds UV-mapped output regions.

4. **Quick presets**: Common setups (2x 16:9, 4:3 + 16:9, 2x 4:3) can be applied with a single click.

---

## Performance Considerations

1. **GPU Upload**: `write_texture` for CPU→GPU transfers
2. **Readback**: Buffered GPU→CPU for NDI output (async)
3. **Syphon**: Zero-copy GPU texture sharing via IOSurface (macOS)
4. **VSync**: Configurable per window
5. **Frame Skip**: NDI output can drop frames to maintain render FPS

---

## Future Extensions

1. **Spout**: Windows GPU texture sharing (Spout2)
2. **v4l2loopback**: Linux virtual video device output
3. **MIDI/OSC**: External controller support
4. **Recording**: GPU-accelerated video recording
5. **Multi-output**: Multiple NDI outputs at different resolutions
6. **Mesh Warping**: Projection mapping geometry correction
7. **Multiple Matrix Outputs**: Support multiple independent video matrices
