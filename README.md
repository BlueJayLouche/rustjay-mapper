# Rusty Mapper

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

#### OpenCV + libclang (for Video Wall calibration)

OpenCV is optional but recommended for the Video Wall auto-calibration feature. It provides more robust ArUco marker detection. If not installed, the app will use an embedded marker dictionary.

**macOS:**
```bash
# Install OpenCV and LLVM (includes libclang)
brew install opencv llvm

# Set environment variables for the build
export LIBCLANG_PATH="/usr/local/opt/llvm/lib"
export DYLD_LIBRARY_PATH="/usr/local/opt/llvm/lib:$DYLD_LIBRARY_PATH"

# Build with OpenCV support
cargo build --release --features opencv
```

**Linux (Ubuntu/Debian):**
```bash
# Install OpenCV and libclang
sudo apt-get update
sudo apt-get install libopencv-dev libclang-dev

# Build with OpenCV support
cargo build --release --features opencv
```

**Linux (Fedora/RHEL):**
```bash
# Install OpenCV and libclang
sudo dnf install opencv-devel clang-devel

# Build with OpenCV support
cargo build --release --features opencv
```

**Windows:**
```powershell
# Install OpenCV using vcpkg
vcpkg install opencv4[core,imgproc] --triplet x64-windows

# Install LLVM for libclang
# Download from: https://github.com/llvm/llvm-project/releases
# Add LLVM to your PATH or set LIBCLANG_PATH environment variable

# Set environment variable (in PowerShell)
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# Build with OpenCV support
cargo build --release --features opencv
```

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

### OpenCV / libclang Issues

**Error: `libclang shared library is not loaded on this thread`**

This error occurs when libclang cannot be found during the OpenCV build.

**Solution for macOS:**
```bash
# Find where libclang is installed
brew list llvm | grep libclang

# Set the environment variable (add to ~/.zshrc or ~/.bash_profile)
export LIBCLANG_PATH="/usr/local/opt/llvm/lib"
export DYLD_LIBRARY_PATH="/usr/local/opt/llvm/lib:$DYLD_LIBRARY_PATH"

# Reload shell configuration
source ~/.zshrc  # or source ~/.bash_profile
```

**Solution for Linux:**
```bash
# Find libclang
find /usr -name "libclang.so*" 2>/dev/null

# Set the environment variable (adjust path as needed)
export LIBCLANG_PATH="/usr/lib/llvm-14/lib"
```

**Solution for Windows:**
```powershell
# Set environment variable in PowerShell
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# Or set permanently via System Properties > Environment Variables
```

**Error: `OpenCV not found`**

Make sure OpenCV is installed and `pkg-config` can find it:

```bash
# macOS
brew install opencv pkg-config
pkg-config --cflags --libs opencv4

# Linux
sudo apt-get install libopencv-dev pkg-config
pkg-config --cflags --libs opencv4
```

**Build without OpenCV (fallback mode):**

If you cannot install OpenCV, the app will work with an embedded marker dictionary:

```bash
cargo build --release --no-default-features
```

Note: Video wall calibration will use fallback marker generation which has slightly different marker patterns but is fully functional.

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

## License

MIT
