# RustJay Mapper Examples

Example programs demonstrating various features.

## Calibration Pattern Display (`aruco_display.rs`)

Generates and displays calibration patterns for video wall setup.

```bash
cargo run --example aruco_display
```

### Command Line Options

```bash
# 2x2 grid (default)
cargo run --example aruco_display -- --grid 2x2

# 3x3 grid
cargo run --example aruco_display -- --grid 3x3

# 4x4 grid with 4K resolution
cargo run --example aruco_display -- --grid 4x4 --resolution 3840x2160
```

### Controls

| Key | Action |
|-----|--------|
| `SPACE` / `N` | Next pattern |
| `P` | Previous pattern |
| `A` | Enable auto-cycle (2 second intervals) |
| `S` | Stop auto-cycle |
| `F` | Toggle fullscreen |
| `ESC` | Exit |

## Video Wall Render (`videowall_render.rs`)

Demonstrates the video wall rendering pipeline.

## Calibration Test (`calibration_test.rs`)

Exercises the calibration state machine.
