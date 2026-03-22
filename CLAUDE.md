# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

RustJay Mapper — a projection mapping video application in Rust with GPU-accelerated rendering (wgpu), NDI and Syphon I/O, and AprilTag-based video wall calibration. macOS is the primary platform.

## Build & Run

```bash
cargo build --release                    # standard build
cargo build --release --no-default-features  # without webcam support
cargo run --release                      # run (two windows: control + fullscreen output)
```

### External SDK requirements

- **Syphon**: `syphon-rs` must exist as a sibling repo at `../syphon-rs/syphon-lib/Syphon.framework` (or override with `SYPHON_FRAMEWORK_DIR`)
- **NDI SDK**: probed at standard macOS paths (or override with `NDI_SDK_DIR`)
- `build.rs` embeds rpaths for both frameworks

### Tests

```bash
cargo test --lib videowall::apriltag     # apriltag-specific tests
cargo test                               # all tests
```

### macOS app bundle

```bash
./build-macos-app.sh                     # builds standalone .app bundle
```

## Architecture

**Dual-window**: control window (ImGui UI) + fullscreen output window, sharing a single `wgpu::Instance` with `Arc<Device>` and `Arc<Queue>`.

**SharedState** (`src/core/state.rs`): `Arc<Mutex<SharedState>>` is the central thread-safe state shared across all threads (main, NDI input, NDI output, audio).

**Threading model**:
- Main thread: event loop, GPU rendering, shared state updates
- NDI input thread (per source): receives BGRA frames → bounded channel (capacity=5, latest-frame-only)
- NDI output thread (singleton): bounded channel (capacity=2), `Box::leak` pattern to persist thread
- Audio thread: cpal callback → 8-band FFT → shared state

**Input system** (`src/input/`): `InputManager` manages two independent input slots. Sources (Webcam, NDI, Syphon) are hot-swappable via `InputCommand` enum written to SharedState, processed by `app/commands.rs` each frame.

**Output system** (`src/output/` + `src/ndi/`): `OutputManager` coordinates NDI sender thread and Syphon GPU output. GPU readback (`output/readback.rs`) bridges GPU→CPU for NDI.

**Shader pipeline** (`src/engine/`): `WgpuEngine` runs the render pipeline. Main shader (`engine/shaders/main.wgsl`) handles corner pinning, UV transforms, blend modes. All textures use `Bgra8Unorm` (native macOS format).

**Video wall** (`src/videowall/`): N×M grid subdivision with AprilTag calibration. Markers generated at runtime via `apriltag_sys` FFI (no pre-generated files). Detection infers aspect ratio and orientation from tag distortion.

## Key patterns

- **Command dispatch**: GUI writes command enums to SharedState → `app/commands.rs` dispatches them next frame
- **Bounded channels with drop semantics**: input queues (capacity=5) and output queues (capacity=2) drop old frames to prevent memory growth and prioritise freshness
- **Error handling**: `anyhow::Result` for fallible operations
- **Logging**: `log` crate macros; run with `RUST_LOG=info` for diagnostics

## Design documents

Refer to these for detailed architecture decisions:
- `DESIGN.md` — full architecture, modules, threading, data flow
- `DESIGN_GUI_LAYOUT.md` — visual GUI layout specification
- `DESIGN_LOCAL_OUTPUT.md` — Syphon/Spout/v4l2loopback output design
- `DESIGN_LOCAL_INPUT.md` — Syphon/Spout/v4l2loopback input design
- `AGENTS.md` — guide for AI agents (dependency table, code style, input commands)
