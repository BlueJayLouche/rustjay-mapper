# RustJay Mapper - Agent Guide

## Project Overview

A **projection mapping video application** built in Rust with:
- NDI and Syphon input/output for video streaming
- GPU-accelerated rendering via wgpu
- Dual-window architecture (control + fullscreen output)
- AprilTag-based video wall calibration
- Hidden cursor on output window for clean projection

## Key Architecture Patterns

### Multi-Input Support (`InputManager`)

**Unified Input Interface** (`InputSource`):
- Supports Webcam, NDI, OBS (via NDI), and Syphon (macOS)
- Each input has independent type and configuration
- Hot-swappable without application restart
- Latest-frame-only semantics for all types

**InputManager** (`src/input/mod.rs`):
- Manages Input 1 and Input 2
- Background device discovery (webcam, NDI); Syphon discovery is inline
- Handles Webcam via nokhwa, NDI via grafton-ndi, Syphon via syphon-core

### NDI Integration

**Input (`NdiReceiver` in `input/ndi.rs`)**:
- Dedicated thread per source
- Receives BGRA, converts to RGBA
- Bounded channel (capacity=5), drops old frames

**Output (`NdiOutputSender` in `ndi/output.rs`)**:
- Dedicated sender thread (prevents render loop blocking)
- Bounded channel (capacity=2) for low latency
- Thread persists via `Box::leak` pattern

### Syphon Integration (macOS)

**Input (`syphon_input.rs`)**: Receives GPU textures from other apps (Resolume, TouchDesigner, etc.) via IOSurface — zero-copy.

**Output (`output/syphon.rs`)**: Shares the rendered output texture with other apps via Syphon server.

### Dual Window Setup

```rust
// Shared wgpu resources (wrapped in Arc)
let (device, queue) = adapter.request_device(...).await?;

// Output window - owns surface
let output_surface = instance.create_surface(output_window)?;

// Control window - shares device/queue via Arc
let imgui_renderer = ImGuiRenderer::new(device, queue, control_window)?;
```

## Important Dependencies

| Crate | Purpose |
|-------|---------|
| `winit` | Window management |
| `wgpu` | GPU rendering |
| `grafton-ndi` | NDI video I/O |
| `syphon-core` / `syphon-wgpu` | Syphon I/O (macOS) |
| `apriltag` / `apriltag-sys` | AprilTag detection + runtime marker generation |
| `crossbeam` | Thread channels |
| `imgui` | UI framework |
| `image` | Image loading/manipulation |
| `rfd` | Native file dialogs |

## Build Notes

- Uses Rust 2021 edition
- Release profile optimised for performance (`lto = "fat"`)
- Requires NDI SDK runtime for NDI functionality
- Requires `syphon-rs` sibling repo for Syphon (macOS) — see README
- `build.rs` handles rpath embedding for Syphon and NDI on macOS

## Code Style

- Module documentation at top of each file
- Thread safety: `Arc<Mutex<T>>` for shared state
- Error handling: `anyhow::Result` for fallible operations
- Logging: `log` crate macros (`info!`, `warn!`, `error!`)

## Input Commands

Input changes are handled via command variants in `SharedState`:

```rust
pub enum InputCommand {
    StartWebcam { device_index, width, height, fps },
    StartNdi { source_name },
    StartSyphon { server_name },
    StopInput,
}
```

GUI sets the command; `App::apply_input_command(slot, cmd)` processes it on the next frame.

## Video Matrix / Grid Mapping

### Concept
- Input texture is subdivided into a configurable N x M grid (e.g. 3x3, 4x4)
- Each grid cell can be mapped to a position in the output
- Output is a single fullscreen window sent to an HDMI video matrix
- Physical displays receive their portion from the matrix

### AprilTag Calibration
- Markers are **generated at runtime** via `apriltag_sys::apriltag_to_image()` FFI — no pre-generated PNG files needed
- The pattern grid adapts to the user's configured grid size (up to 9x9)
- Detection infers aspect ratio (4:3, 16:9, 21:9) from tag distortion and builds UV-mapped output regions
- Quick presets for common setups (2x 16:9, 4:3 + 16:9, etc.)

### Key Files
| File | Purpose |
|------|---------|
| `videowall/apriltag.rs` | `AprilTagDetector` + `AprilTagGenerator` (runtime FFI generation) |
| `videowall/apriltag_auto_detect.rs` | Screen layout auto-detection from photo or live input |
| `videowall/grid_mapping.rs` | Grid subdivision and cell-to-output mapping |
| `videowall/matrix_renderer.rs` | GPU renderer for the grid output |

## Testing

### NDI
Install NDI Tools for testing: NDI Video Monitor, NDI Test Patterns.

### Webcam
Webcam support is optional via the `webcam` feature (enabled by default):
```bash
# With webcam (default)
cargo run

# Without webcam
cargo run --no-default-features
```

### AprilTag
```bash
# Run apriltag-specific tests
cargo test --lib videowall::apriltag
```
