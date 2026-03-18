# Rusty Mapper - Design Document

## Overview

A high-performance Rust video application for projection mapping with NDI input/output, dual-window architecture (preview/control + fullscreen output), and GPU-accelerated rendering via wgpu.

## Goals

- **High Performance**: 60fps+ rendering with minimal latency
- **Dual Window Architecture**: Control window for UI, fullscreen output window with hidden cursor
- **NDI Integration**: Both input (receive) and output (send) with dedicated threads
- **Cross-Platform**: macOS primary, with Linux/Windows support potential
- **Projection Mapping Ready**: Fullscreen output, cursor hiding, configurable resolutions

---

## Architecture

### High-Level Structure

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RUSTY MAPPER                                       │
│                                                                              │
│  ┌─────────────────────┐      ┌─────────────────────────────────────────┐   │
│  │   CONTROL WINDOW    │      │          OUTPUT WINDOW                   │   │
│  │  (imgui + preview)  │      │      (Fullscreen, No Cursor)             │   │
│  │                     │      │                                          │   │
│  │  ┌───────────────┐  │      │  ┌────────────────────────────────────┐  │   │
│  │  │ NDI Source    │  │      │  │       RENDER PIPELINE               │  │   │
│  │  │ Selector      │  │      │  │  ┌──────────┐ ┌──────────┐         │  │   │
│  │  ├───────────────┤  │      │  │  │  Input   │ │ Effects │         │  │   │
│  │  │ Preview       │  │◄─────┼──┼──┤ Processor│ │  Stage  │         │  │   │
│  │  │ (320x180)     │  │      │  │  └────┬─────┘ └────┬─────┘         │  │   │
│  │  ├───────────────┤  │      │  │       └────────────┘                │  │   │
│  │  │ Output Ctrl   │  │      │  │            ↓                        │  │   │
│  │  │ (NDI/Window)  │  │      │  │  ┌──────────────────────┐           │  │   │
│  │  └───────────────┘  │      │  │  │    Output Mixer      │           │  │   │
│  │                     │      │  │  │  (Projection Mapped) │           │  │   │
│  │  ┌───────────────┐  │      │  │  └──────────┬───────────┘           │  │   │
│  │  │ Parameters    │  │      │  │             ↓                       │  │   │
│  │  │ (Real-time)   │◄─┼──────┼──┼─────────────┘                       │  │   │
│  │  └───────────────┘  │      │  └────────────────────────────────────┘  │   │
│  └─────────────────────┘      └─────────────────────────────────────────┘   │
│           ▲                              │                                  │
│           │                              │                                  │
│           │         ┌────────────────────┴────────────────┐                 │
│           │         │           SHARED STATE               │                 │
│           │         │  (Parameters, Audio, NDI Sources)    │                 │
│           │         └───────────────────────────────────────┘                 │
│           │                                                                   │
│  ┌────────┴─────────────────────────┐     ┌──────────────────────────────┐  │
│  │      NDI INPUT THREAD            │     │    NDI OUTPUT THREAD         │  │
│  │  ┌────────────┐  ┌─────────────┐ │     │  ┌────────────────────────┐  │  │
│  │  │ Receiver   │──│ Frame Queue │ │     │  │   Frame Queue (2)      │  │  │
│  │  │ (BGRA→RGBA)│  │ (latest-only)│     │  │                        │  │  │
│  │  └────────────┘  └──────┬──────┘ │     │  │  ┌──────────┐ ┌──────┐ │  │  │
│  │                         │        │     │  │  │ Sender   │ │ NDI  │ │  │  │
│  └─────────────────────────┘        │     │  │  │ (RGBA→   │ │ Send │ │  │  │
│                                     │     │  │  │  BGRA)   │ │      │ │  │  │
│                                     │     │  │  └──────────┘ └──────┘ │  │  │
│                                     │     │  └────────────────────────┘  │  │
│                                     │     └──────────────────────────────┘  │
│                                     │                                        │
│  ┌─────────────────────────────────┴──────────────┐                         │
│  │              AUDIO INPUT                         │                         │
│  │  ┌────────────┐  ┌─────────────┐  ┌──────────┐ │                         │
│  │  │ cpal Input │──│ FFT (8-band)│──│ Shared   │─┘                         │
│  │  └────────────┘  └─────────────┘  │   State  │                            │
│  └───────────────────────────────────┴──────────┘                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Modules

### 1. Core Module (`src/core/`)
- **SharedState**: Thread-safe state shared between windows and threads
- **Parameters**: Real-time adjustable parameters (LFOs, audio modulation)
- **Vertex**: GPU vertex definitions for quad rendering

### 2. Application (`src/app/` + `winit`)
Dual-window application handler implementing `winit::application::ApplicationHandler`, split into focused sub-modules following rustjay-template conventions:
- **`mod.rs`**: `App` struct definition, `run_app()` entry point, `toggle_fullscreen()`
- **`commands.rs`**: `dispatch_commands()` — processes `InputCommand` and `NdiOutputCommand` each frame; `apply_input_command(slot, cmd)` helper eliminates input1/input2 duplication
- **`update.rs`**: `update_inputs()`, `process_videowall_calibration()`, `sync_video_wall_state()`, `sync_video_matrix_state()`, `update_preview_textures()`
- **`events.rs`**: `ApplicationHandler` impl — `resumed()`, `window_event()`, `about_to_wait()`

Window roles:
- **Output Window**: Fullscreen-capable, cursor hidden, wgpu surface
- **Control Window**: ImGui-based UI, resizable, decorated

### 3. NDI Input (`src/ndi/input.rs`)
Based on rustjay_waaaves patterns:
- **NdiSourceFinder**: Network discovery of NDI sources
- **NdiReceiver**: Background thread receiver with BGRA→RGBA conversion
- **Bounded channel** with latest-frame-only semantics (drops old frames)
- Thread-safe frame queue using `crossbeam::channel`

### 4. NDI Output (`src/ndi/output.rs`)
Dedicated sender thread pattern from rustjay_waaaves:
- **NdiOutputSender**: Background thread sender with RGBA→BGRA/BGRX conversion
- **Bounded channel** (capacity=2) - drops frames if consumer is slow
- **Cloneable handle**: Non-owning clones can submit frames
- Thread persists via `Box::leak` pattern

### 5. Rendering Engine (`src/engine/`)
GPU-accelerated pipeline using wgpu:
- **TextureManager**: Input texture management and updates
- **RenderPipeline**: Shader-based video processing
- **Blit Pipeline**: Final output to surface

### 6. GUI (`src/gui/`)
ImGui-based control interface:
- **ControlGui**: Parameter controls, NDI source selection
- **Preview**: Real-time output preview (via shared texture)

### 7. Audio (`src/audio/`)
Optional audio analysis:
- **AudioInput**: cpal-based audio capture
- **FFT**: 8-band frequency analysis
- **Beat Detection**: BPM and phase tracking

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

### Input Flow (Webcam/NDI/OBS)
```
Webcam:  Camera → MJPEG/YUYV → RGBA Conversion → Frame Queue → GPU Upload → Shader
NDI:     Network → NDI SDK → Receiver Thread → BGRA→RGBA → Frame Queue → GPU Upload → Shader
OBS:     OBS NDI Output → Same as NDI above
```

### NDI Output Flow
```
Shader Output → GPU Readback → RGBA→BGRA Conversion → Frame Queue → Sender Thread → NDI SDK → Network
```

---

## Key Design Decisions

### 1. Dedicated NDI Output Thread
Following rustjay_waaaves pattern:
- **Why**: NDI SDK send operations can block; moving off main thread prevents frame drops
- **How**: Thread owns NDI `Sender`, receives frames via channel, uses `Box::leak` to persist
- **Benefit**: Render loop never blocks on network I/O

### 2. Multi-Input Support
- **Input Types**: Webcam (via nokhwa), NDI (Network Device Interface), OBS (via NDI output), Syphon (macOS GPU zero-copy)
- **Independent Mapping**: Each input can be selected independently with its own configuration
- **Hot Swappable**: Change inputs on the fly without restarting the application
- **Refreshable Lists**: Device lists are cached but can be refreshed to detect new sources
- **Command dispatch**: GUI writes an `InputCommand` variant into `SharedState.input1_command` / `input2_command`; `App::apply_input_command(slot, cmd)` processes both slots with a single code path

### 3. Bounded Frame Queues
- **Input Queue**: Capacity 5, drops oldest when full (latest-frame semantics)
- **Output Queue**: Capacity 2, drops when full (low-latency over reliability)
- **Why**: Prevents memory growth under load, prioritizes fresh frames

### 3. Dual Window with Shared GPU Context
- Single `wgpu::Instance`, shared `Device` and `Queue`
- Output window owns surface, control window shares resources
- ImGui renderer uses same device/queue for UI rendering

### 4. Cursor Hiding on Output Window
```rust
// Default hidden
window.set_cursor_visible(false);

// Show when cursor leaves, hide when enters
WindowEvent::CursorEntered { .. } => window.set_cursor_visible(false),
WindowEvent::CursorLeft { .. } => window.set_cursor_visible(true),
```

### 5. Fullscreen Toggle
```rust
fn toggle_fullscreen(&mut self) {
    let fullscreen_mode = if self.output_fullscreen {
        Some(winit::window::Fullscreen::Borderless(None))
    } else {
        None
    };
    output_window.set_fullscreen(fullscreen_mode);
}
```

---

## File Structure

```
rusty_mapper/
├── Cargo.toml
├── build.rs                  # macOS rpath setup: Syphon + NDI (NDI_SDK_DIR / SYPHON_FRAMEWORK_DIR)
├── DESIGN.md                 # This document
├── src/
│   ├── main.rs               # Entry point
│   ├── app/
│   │   ├── mod.rs            # App struct, run_app(), toggle_fullscreen()
│   │   ├── commands.rs       # dispatch_commands(), apply_input_command(slot, cmd)
│   │   ├── update.rs         # update_inputs(), calibration sync, preview textures
│   │   └── events.rs         # ApplicationHandler impl (resumed, window_event, about_to_wait)
│   ├── config.rs             # Configuration loading
│   ├── core/
│   │   ├── mod.rs            # Core module exports
│   │   ├── state.rs          # SharedState, InputCommand, NdiOutputCommand
│   │   └── vertex.rs         # GPU vertex types
│   ├── input/
│   │   ├── mod.rs            # InputManager, InputType
│   │   ├── ndi.rs            # NDI receiver
│   │   ├── syphon_input.rs   # Syphon input (macOS only)
│   │   └── webcam.rs         # Webcam capture (optional feature)
│   ├── ndi/
│   │   ├── mod.rs            # NDI module exports
│   │   └── output.rs         # NdiOutputSender (dedicated output thread)
│   ├── output/
│   │   ├── mod.rs            # Output module exports
│   │   └── syphon.rs         # Syphon output (macOS only)
│   ├── engine/
│   │   ├── mod.rs            # Engine exports
│   │   ├── renderer.rs       # Main wgpu renderer + pipeline
│   │   └── texture.rs        # Texture utilities
│   ├── gui/
│   │   ├── mod.rs            # GUI exports
│   │   ├── gui.rs            # ImGui control interface (tabs: Inputs, Mapping, Matrix, Output, Settings)
│   │   └── renderer.rs       # ImGui wgpu renderer
│   ├── videowall/            # Video wall + matrix subsystem (~6,000 LOC)
│   │   ├── mod.rs
│   │   ├── calibration.rs    # State machine calibration controller
│   │   ├── apriltag.rs       # AprilTag detection (pure Rust)
│   │   ├── apriltag_auto_detect.rs
│   │   ├── aruco.rs          # ArUco detection
│   │   ├── quad_mapper.rs
│   │   ├── grid_mapping.rs
│   │   ├── matrix_renderer.rs
│   │   ├── renderer.rs
│   │   ├── config.rs
│   │   └── test_pattern.rs
│   └── audio/
│       ├── mod.rs            # Audio exports
│       └── input.rs          # cpal capture + 8-band FFT
└── config.toml               # Runtime configuration
```

---

## Dependencies

```toml
[dependencies]
# Windowing & Graphics
winit = "0.30"
wgpu = "25.0"
pollster = "0.3"

# Math
glam = { version = "0.29", features = ["bytemuck"] }
bytemuck = { version = "1.21", features = ["derive"] }

# NDI
grafton-ndi = "0.11"

# Threading
crossbeam = "0.8"

# Audio
cpal = "0.15"
rustfft = "6.2"

# GUI
imgui = "0.12"
imgui-wgpu = "0.25"
imgui-winit-support = "0.13"

# Serialization
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Logging
log = "0.4"
env_logger = "0.11"

# Error Handling
anyhow = "1.0"
thiserror = "2.0"
```

---

## Build Configuration (`build.rs`)

On macOS, `build.rs` embeds runtime rpaths so both Syphon and NDI dylibs are found by `dyld`:

| Library | Discovery | Env var override |
|---------|-----------|-----------------|
| `Syphon.framework` | `<workspace>/../crates/syphon/syphon-lib/` | `SYPHON_FRAMEWORK_DIR` |
| `libndi.dylib` | Probes standard SDK install paths (e.g. `/Library/NDI SDK for Apple/lib/macOS`) | `NDI_SDK_DIR` |

`@executable_path` and `@loader_path` entries are also added to support bundled app deployments.

---

## Performance Considerations

1. **GPU Upload**: Use `write_texture` for CPU→GPU transfers
2. **Readback**: Triple-buffered GPU→CPU for NDI output (async)
3. **VSync**: Configurable (on for output, off for control)
4. **Frame Skip**: NDI output can skip frames to maintain render FPS

---

## Input Device Selection

The GUI provides a device selector window with tabs for different input types:

### Webcam Tab
- Lists available webcam devices (0-3, auto-detected)
- Select any detected camera for Input 1 or Input 2
- Uses nokhwa library with MJPEG decoding

### NDI Tab
- Lists NDI sources on the network (non-OBS)
- Auto-refreshes on window open
- Manual refresh button available

### OBS Tab
- Shows NDI sources with "OBS" in the name
- Requires OBS NDI plugin installed and active
- Direct selection for streaming output

### Device Refresh
- Menu: Devices → Refresh All
- Detects newly connected sources
- Updates cached device lists

## Feature Flags

### Webcam Support
Enabled by default. Disable with:
```bash
cargo build --no-default-features
```

Useful when:
- libclang is not available
- Webcam support is not needed
- Building for headless deployment

## Video Matrix / Grid Mapping

The application supports projection mapping to multiple displays via an HDMI video matrix using a **grid subdivision approach**:

### Architecture

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

### Key Features

- **Configurable Grid**: N×M subdivision of input texture (3×3 default)
- **Per-Cell Mapping**: Each grid cell maps independently to output
- **AprilTag Detection**: Auto-detects aspect ratio and orientation from markers
- **Single Output**: One HDMI output feeds the video matrix
- **Unmapped Cells**: Render as black (no signal)

See [DESIGN_GUI_LAYOUT.md](DESIGN_GUI_LAYOUT.md) for detailed UI design.

## Future Extensions

1. **Syphon/Spout**: macOS/Windows GPU texture sharing
2. **MIDI/OSC**: External controller support
3. **Recording**: GPU-accelerated video recording
4. **Multi-output**: Multiple NDI outputs, different resolutions
5. **Projection Mapping**: Mesh warping for geometry correction
6. **More Input Types**: SDI capture cards, Blackmagic devices
7. **Multiple Matrix Outputs**: Support multiple independent video matrices
