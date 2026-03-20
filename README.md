# RustJay Mapper

A high-performance projection mapping application in Rust with NDI input/output and GPU-accelerated rendering.

## Features

- **Multiple Input Types**:
  - **Webcam**: Direct camera capture via nokhwa
  - **NDI**: Network Device Interface sources
  - **OBS**: OBS Studio output via NDI plugin
- **Independent Input Mapping**: Each input slot can have different source type
- **Refreshable Device Lists**: Hot-swap inputs without restart
- **NDI Input/Output**: Full NDI support with dedicated threads for low-latency video streaming
- **Dual Window Architecture**: 
  - Fullscreen output window with hidden cursor for clean projection
  - Control window with ImGui-based interface
- **GPU Acceleration**: wgpu-based rendering for cross-platform support
- **Projection Mapping**: Corner pinning with bilinear interpolation for quad warping
- **Transform Controls**: Per-input scale, offset, and rotation
- **Blend Modes**: Normal (mix), Add, Multiply, Screen blending
- **Visual GUI**: Preview-centric interface with source/output previews and overlayed display boxes
- **Video Wall Support**: Auto-calibration for HDMI matrix walls via ArUco markers
  - Static pattern calibration (all markers shown simultaneously)
  - Per-display adjustments (brightness, contrast, gamma)
  - Manual corner adjustment (drag-to-move)
  - Named preset save/load
- **Local Sharing**: Syphon/Spout/v4l2loopback for inter-app video sharing
- **Configurable Resolution**: Internal rendering resolution independent of window size

## Architecture

See [DESIGN.md](DESIGN.md) for detailed architecture documentation.

### Design Documents

- [DESIGN_GUI_LAYOUT.md](DESIGN_GUI_LAYOUT.md) - Visual GUI layout with source/output previews
- [DESIGN_VIDEOWALL.md](DESIGN_VIDEOWALL.md) - Video wall auto-calibration using ArUco markers
- [DESIGN_LOCAL_OUTPUT.md](DESIGN_LOCAL_OUTPUT.md) - Local video output (Syphon/Spout/v4l2loopback)
- [DESIGN_LOCAL_INPUT.md](DESIGN_LOCAL_INPUT.md) - Local video input (Syphon/Spout/v4l2loopback)

```
┌─────────────────┐      ┌─────────────────────┐
│  Control Window │      │   Output Window     │
│   (ImGui UI)    │      │  (Fullscreen, No    │
│                 │      │   Cursor)           │
└────────┬────────┘      └──────────┬──────────┘
         │                          │
         └──────────┬───────────────┘
                    │
           ┌────────▼────────┐
           │   wgpu Engine   │
           │  (GPU Render)   │
           └────────┬────────┘
                    │
        ┌───────────┴───────────┐
        │                       │
┌───────▼────────┐      ┌───────▼────────┐
│  NDI Input     │      │  NDI Output    │
│   Thread       │      │   Thread       │
└────────────────┘      └────────────────┘
```

## Building

### Requirements

- Rust 1.70+
- NDI SDK (for NDI support)
- OpenCV 4.x + libclang (optional, for enhanced ArUco marker detection)
- macOS, Linux, or Windows

### Optional Dependencies

#### OpenCV (NOT CURRENTLY SUPPORTED)

**Status:** OpenCV 4.13+ has compatibility issues with the `opencv` Rust crate. The Video Wall feature works perfectly without OpenCV using the embedded ArUco marker dictionary.

**Recommendation:** Use the default build without OpenCV. The embedded dictionary provides equivalent marker detection quality.

If you need OpenCV support in the future, you would need to install OpenCV 4.8 or earlier:

**macOS (OpenCV 4.8 - NOT recommended due to compatibility):**
```bash
# Install older OpenCV version
brew install opencv@4

# Set environment variables
export LIBCLANG_PATH="/usr/local/opt/llvm/lib"
export DYLD_LIBRARY_PATH="$LIBCLANG_PATH:$DYLD_LIBRARY_PATH"

# Build with OpenCV support (feature currently disabled)
# cargo build --release --features opencv
```

**Note:** The Video Wall calibration feature uses an embedded ArUco DICT_4X4_50 dictionary that provides the same marker patterns as OpenCV. No functionality is lost by using the default build.

### Build

**Standard build (without OpenCV):**
```bash
cargo build --release
```

**Build with all features (OpenCV + webcam):**
```bash
cargo build --release --features "opencv webcam"
```

**Build without default features (minimal):**
```bash
cargo build --release --no-default-features
```

### Run

```bash
cargo run --release
```

**Run with OpenCV features:**
```bash
# macOS/Linux
export LIBCLANG_PATH="/usr/local/opt/llvm/lib"
cargo run --release --features opencv

# Windows
set LIBCLANG_PATH=C:\Program Files\LLVM\bin
cargo run --release --features opencv
```

### Syphon Support (macOS Only)

Syphon is enabled automatically on macOS. The build system finds the framework at `../syphon-rs/syphon-lib/Syphon.framework`.

**Requirements:** The `syphon-rs` repo must be present as a sibling directory:
```
developer/rust/
├── syphon-rs/          ← must exist
└── rustjay-mapper/
```

If your layout differs:
```bash
SYPHON_FRAMEWORK_DIR=/path/to/syphon-rs/syphon-lib cargo build --release
```

## Usage

1. **Start the application** - Two windows will appear:
   - Output window (main display)
   - Control window (settings)

2. **Select Input Sources** (Inputs tab):
   - Click "Select Source" for Input 1 or Input 2
   - Choose from Webcam, NDI, or OBS tabs
   - Click "Refresh" to detect new sources

3. **Configure Mapping** (Mapping tab):
   - Select Input 1 or Input 2 to map
   - Adjust corner positions for projection mapping
   - Set scale, offset, and rotation
   - Choose blend mode and opacity

4. **Start NDI Output** (Output tab):
   - Enter a stream name
   - Click "Start NDI Output"

5. **Toggle Fullscreen**:
   - Press `Shift+F` in the output window
   - Or use the checkbox in the Output tab

6. **Exit**:
   - Press `Escape` in the output window
   - Or close either window

### OBS Integration

To use OBS as an input source:
1. Install OBS NDI plugin: https://github.com/obs-ndi/obs-ndi
2. In OBS: Tools → NDI Output Settings → Enable
3. In Rusty Mapper: Select "OBS" tab in input selector
4. Choose your OBS NDI source

## Configuration

Edit `config.toml` to customize:

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
├── main.rs          # Entry point
├── app.rs           # Dual-window application handler
├── config.rs        # Configuration loading
├── core/            # Core types and shared state
├── input/           # Input management (webcam, NDI, OBS)
│   ├── mod.rs       # InputManager, InputSource
│   ├── ndi.rs       # NDI receiver
│   └── webcam.rs    # Webcam capture
├── ndi/             # NDI output
├── engine/          # wgpu rendering engine
├── gui/             # ImGui control interface
└── audio/           # Audio input (optional)
```

## Troubleshooting

### OpenCV / libclang Issues (OpenCV feature currently disabled)

**Note:** OpenCV support is currently disabled due to compatibility issues with OpenCV 4.13+. The Video Wall feature works perfectly using the embedded ArUco marker dictionary.

**Recommended solution:** Use the default build without OpenCV:
```bash
cargo build --release
```

The embedded dictionary provides equivalent marker detection quality. If you previously tried to build with OpenCV and are seeing errors:

```bash
# Clean the build cache
cargo clean

# Build without OpenCV
cargo build --release
```

**If you need OpenCV in the future:**
OpenCV 4.13+ has API changes that are incompatible with the current `opencv` Rust crate. You would need OpenCV 4.8 or earlier, which is not easily available via Homebrew. The embedded dictionary is the recommended solution.

### NDI SDK Not Found

Download and install the NDI SDK from:
https://ndi.video/tools-sdk/

After installation, you may need to set:
```bash
# macOS/Linux
export NDI_SDK_DIR="/path/to/NDI/sdk"

# Windows
set NDI_SDK_DIR=C:\Program Files\NDI\NDI 5 SDK
```

### Syphon Framework Not Found

If you see `dyld: Library not loaded: Syphon.framework`:
1. Verify: `ls ../syphon-rs/syphon-lib/Syphon.framework`
2. Override: `SYPHON_FRAMEWORK_DIR=/path/to/syphon-rs/syphon-lib cargo build --release`

## License

MIT
