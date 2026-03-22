//! # Grid Cell Mapping
//!
//! Grid-based video matrix mapping system.
//! Subdivides input texture into N×M grid cells that can be mapped to output positions.
//!
//! ## Key Concepts
//!
//! - **Input Grid**: Input texture is subdivided into a configurable N×M grid (e.g., 3×3)
//! - **Cell Mapping**: Each grid cell can be mapped to a position in the output
//! - **AprilTag Detection**: Auto-detects aspect ratio and orientation from markers
//! - **Black Cells**: Unmapped cells render as black (no signal)
//!
//! ## Example
//!
//! ```rust,ignore
//! use rusty_mapper::videowall::{InputGridConfig, GridCellMapping, GridSize};
//!
//! // Configure 3×3 input grid
//! let input_grid = InputGridConfig::new(GridSize::new(3, 3));
//!
//! // Map input cell 0 (top-left) to output position (0, 0) with 4:3 aspect
//! let mapping = GridCellMapping::new(0, GridPosition::new(0, 0, 1, 1))
//!     .with_aspect_ratio(AspectRatio::Ratio4_3)
//!     .with_orientation(Orientation::Normal);
//! ```

use super::{GridSize, Rect};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Aspect ratio for a mapped display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    /// 4:3 standard
    Ratio4_3,
    /// 16:9 widescreen
    Ratio16_9,
    /// 16:10 computer
    Ratio16_10,
    /// 1:1 square
    Ratio1_1,
    /// 21:9 ultrawide
    Ratio21_9,
    /// Custom aspect ratio
    Custom { w: u32, h: u32 },
}

impl AspectRatio {
    /// Get the aspect ratio as a float (width / height)
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Ratio4_3 => 4.0 / 3.0,
            Self::Ratio16_9 => 16.0 / 9.0,
            Self::Ratio16_10 => 16.0 / 10.0,
            Self::Ratio1_1 => 1.0,
            Self::Ratio21_9 => 21.0 / 9.0,
            Self::Custom { w, h } => *w as f32 / *h.max(&1) as f32,
        }
    }

    /// Get a human-readable name
    pub fn name(&self) -> String {
        match self {
            Self::Ratio4_3 => "4:3".to_string(),
            Self::Ratio16_9 => "16:9".to_string(),
            Self::Ratio16_10 => "16:10".to_string(),
            Self::Ratio1_1 => "1:1".to_string(),
            Self::Ratio21_9 => "21:9".to_string(),
            Self::Custom { w, h } => format!("{}:{}", w, h),
        }
    }

    /// Detect aspect ratio from width and height
    pub fn detect(width: f32, height: f32) -> Self {
        if width <= 0.0 || height <= 0.0 {
            return Self::Ratio16_9; // Default
        }
        let ratio = width / height;
        
        // Find closest match
        let ratios = [
            (Self::Ratio4_3, 4.0 / 3.0),
            (Self::Ratio16_9, 16.0 / 9.0),
            (Self::Ratio16_10, 16.0 / 10.0),
            (Self::Ratio1_1, 1.0),
            (Self::Ratio21_9, 21.0 / 9.0),
        ];
        
        let mut closest = Self::Ratio16_9;
        let mut min_diff = f32::MAX;
        
        for (ar, val) in ratios {
            let diff = (ratio - val).abs();
            if diff < min_diff {
                min_diff = diff;
                closest = ar;
            }
        }
        
        // If close to a standard ratio, use it; otherwise custom
        if min_diff < 0.1 {
            closest
        } else {
            Self::Custom { 
                w: (width * 100.0) as u32, 
                h: (height * 100.0) as u32 
            }
        }
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::Ratio16_9
    }
}

/// Orientation of a display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Normal orientation (0°)
    Normal,
    /// Rotated 90° clockwise
    Rotated90,
    /// Rotated 180°
    Rotated180,
    /// Rotated 270° clockwise (or 90° counter-clockwise)
    Rotated270,
}

impl Orientation {
    /// Get rotation angle in degrees
    pub fn degrees(&self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::Rotated90 => 90,
            Self::Rotated180 => 180,
            Self::Rotated270 => 270,
        }
    }

    /// Get rotation angle in radians
    pub fn radians(&self) -> f32 {
        (self.degrees() as f32).to_radians()
    }

    /// Detect orientation from marker corners
    /// 
    /// AprilTag library returns corners in image coordinates (Y increases downward):
    /// [0]=left-bottom, [1]=right-bottom, [2]=right-top, [3]=left-top
    /// The top edge goes from corners[3] (left-top) to corners[2] (right-top)
    pub fn detect_from_corners(corners: &[[f32; 2]; 4]) -> Self {
        // Calculate the orientation from the top edge (corners[3] to corners[2])
        // This is left-to-right in a normal (non-rotated) tag
        let top_edge = Vec2::new(
            corners[2][0] - corners[3][0],  // RT.x - LT.x
            corners[2][1] - corners[3][1],  // RT.y - LT.y
        );
        
        let angle = top_edge.y.atan2(top_edge.x);
        let degrees = angle.to_degrees();
        
        // Normalize to 0-360
        let normalized = ((degrees % 360.0) + 360.0) % 360.0;
        
        // Find closest orientation
        if normalized < 45.0 || normalized >= 315.0 {
            Self::Normal
        } else if normalized < 135.0 {
            Self::Rotated90
        } else if normalized < 225.0 {
            Self::Rotated180
        } else {
            Self::Rotated270
        }
    }

    /// Apply orientation to UV coordinates
    /// Returns the rotated UV coordinate
    pub fn apply_to_uv(&self, uv: Vec2) -> Vec2 {
        match self {
            Self::Normal => uv,
            Self::Rotated90 => Vec2::new(1.0 - uv.y, uv.x),
            Self::Rotated180 => Vec2::new(1.0 - uv.x, 1.0 - uv.y),
            Self::Rotated270 => Vec2::new(uv.y, 1.0 - uv.x),
        }
    }
}

impl Default for Orientation {
    fn default() -> Self {
        Self::Normal
    }
}

/// Position in the output grid
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridPosition {
    /// Column in output grid
    pub col: f32,
    /// Row in output grid
    pub row: f32,
    /// Width in grid cells (can be fractional)
    pub width: f32,
    /// Height in grid cells (can be fractional)
    pub height: f32,
}

impl GridPosition {
    /// Create a new grid position
    pub fn new(col: f32, row: f32, width: f32, height: f32) -> Self {
        Self {
            col,
            row,
            width,
            height,
        }
    }

    /// Get the center position
    pub fn center(&self) -> (f32, f32) {
        (self.col + self.width / 2.0, self.row + self.height / 2.0)
    }

    /// Get corners in normalized coordinates (0-1)
    /// Given total grid dimensions
    pub fn to_normalized_rect(&self, total_cols: u32, total_rows: u32) -> Rect {
        let total_c = total_cols.max(1) as f32;
        let total_r = total_rows.max(1) as f32;
        
        Rect::new(
            self.col / total_c,
            self.row / total_r,
            self.width / total_c,
            self.height / total_r,
        )
    }
}

impl Default for GridPosition {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0, 1.0)
    }
}

/// Mapping from an input grid cell to an output position
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridCellMapping {
    /// Input grid cell index (0 = top-left, row-major)
    pub input_cell: usize,
    
    /// Output grid position
    pub output_position: GridPosition,
    
    /// Aspect ratio (auto-detected or manual)
    pub aspect_ratio: AspectRatio,
    
    /// Orientation (auto-detected or manual)
    pub orientation: Orientation,
    
    /// Whether this mapping is enabled
    pub enabled: bool,
    
    /// Display ID for reference (optional)
    pub display_id: Option<u32>,
    
    /// Custom source rectangle for auto-detected regions
    /// If set, overrides the grid-based source calculation
    pub custom_source_rect: Option<Rect>,
}

impl GridCellMapping {
    /// Create a new mapping
    pub fn new(input_cell: usize, output_position: GridPosition) -> Self {
        Self {
            input_cell,
            output_position,
            aspect_ratio: AspectRatio::default(),
            orientation: Orientation::default(),
            enabled: true,
            display_id: None,
            custom_source_rect: None,
        }
    }

    /// Set aspect ratio
    pub fn with_aspect_ratio(mut self, ratio: AspectRatio) -> Self {
        self.aspect_ratio = ratio;
        self
    }

    /// Set orientation
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set display ID
    pub fn with_display_id(mut self, id: u32) -> Self {
        self.display_id = Some(id);
        self
    }
    
    /// Set custom source rectangle (for auto-detected regions)
    pub fn with_source_rect(mut self, rect: Rect) -> Self {
        self.custom_source_rect = Some(rect);
        self
    }

    /// Get source rectangle in the input texture
    /// Given the input grid dimensions
    pub fn get_source_rect(&self, input_grid: GridSize) -> Rect {
        // Use custom source rect if available (from auto-detection)
        if let Some(rect) = self.custom_source_rect {
            return rect;
        }
        
        // Otherwise calculate from grid cell
        let cell_col = (self.input_cell % input_grid.columns as usize) as f32;
        let cell_row = (self.input_cell / input_grid.columns as usize) as f32;
        let cols = input_grid.columns as f32;
        let rows = input_grid.rows as f32;
        
        Rect::new(
            cell_col / cols,
            cell_row / rows,
            1.0 / cols,
            1.0 / rows,
        )
    }

    /// Get destination rectangle in normalized output coordinates
    /// Given the output grid dimensions
    pub fn get_dest_rect(&self, output_grid: GridSize) -> Rect {
        self.output_position.to_normalized_rect(
            output_grid.columns,
            output_grid.rows,
        )
    }
}

/// Configuration for the input texture grid subdivision
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputGridConfig {
    /// Grid dimensions (e.g., 3×3)
    pub grid_size: GridSize,
    
    /// Input source (1 or 2)
    pub input_source: u8,
    
    /// Cell mappings
    pub mappings: Vec<GridCellMapping>,
}

impl InputGridConfig {
    /// Create a new input grid config
    pub fn new(grid_size: GridSize) -> Self {
        Self {
            grid_size,
            input_source: 1,
            mappings: Vec::new(),
        }
    }

    /// Set input source
    pub fn with_input_source(mut self, source: u8) -> Self {
        self.input_source = source.clamp(1, 2);
        self
    }

    /// Add a cell mapping
    pub fn add_mapping(&mut self, mapping: GridCellMapping) {
        self.mappings.push(mapping);
    }

    /// Remove a mapping by input cell
    pub fn remove_mapping(&mut self, input_cell: usize) -> Option<GridCellMapping> {
        if let Some(idx) = self.mappings.iter().position(|m| m.input_cell == input_cell) {
            Some(self.mappings.remove(idx))
        } else {
            None
        }
    }

    /// Get mapping for input cell
    pub fn get_mapping(&self, input_cell: usize) -> Option<&GridCellMapping> {
        self.mappings.iter().find(|m| m.input_cell == input_cell)
    }

    /// Get mutable mapping for input cell
    pub fn get_mapping_mut(&mut self, input_cell: usize) -> Option<&mut GridCellMapping> {
        self.mappings.iter_mut().find(|m| m.input_cell == input_cell)
    }

    /// Clear all mappings
    pub fn clear_mappings(&mut self) {
        self.mappings.clear();
    }

    /// Get total number of cells
    pub fn total_cells(&self) -> usize {
        (self.grid_size.columns * self.grid_size.rows) as usize
    }

    /// Get cell position from index
    pub fn cell_position(&self, index: usize) -> (u32, u32) {
        let col = (index % self.grid_size.columns as usize) as u32;
        let row = (index / self.grid_size.columns as usize) as u32;
        (col, row)
    }

    /// Get cell index from position
    pub fn cell_index(&self, col: u32, row: u32) -> usize {
        (row * self.grid_size.columns + col) as usize
    }

    /// Get unmapped cells (available for mapping)
    pub fn unmapped_cells(&self) -> Vec<usize> {
        let total = self.total_cells();
        let mapped: std::collections::HashSet<_> = 
            self.mappings.iter().map(|m| m.input_cell).collect();
        
        (0..total).filter(|i| !mapped.contains(i)).collect()
    }

    /// Check if a cell is mapped
    pub fn is_cell_mapped(&self, input_cell: usize) -> bool {
        self.mappings.iter().any(|m| m.input_cell == input_cell)
    }

    /// Create a default mapping where each cell maps to corresponding output position
    /// For a 3×3 grid: cell 0 → (0,0), cell 1 → (1,0), etc.
    pub fn create_default_mapping(&mut self) {
        self.mappings.clear();
        let total = self.total_cells();
        
        for i in 0..total {
            let (col, row) = self.cell_position(i);
            let mapping = GridCellMapping::new(
                i,
                GridPosition::new(col as f32, row as f32, 1.0, 1.0),
            );
            self.mappings.push(mapping);
        }
    }

    /// Calculate output grid size needed for all mappings
    pub fn calculate_output_grid_size(&self) -> GridSize {
        let mut max_col = 0u32;
        let mut max_row = 0u32;
        
        for mapping in &self.mappings {
            let end_col = (mapping.output_position.col + mapping.output_position.width).ceil() as u32;
            let end_row = (mapping.output_position.row + mapping.output_position.height).ceil() as u32;
            max_col = max_col.max(end_col);
            max_row = max_row.max(end_row);
        }
        
        GridSize::new(max_col.max(1), max_row.max(1))
    }
}

impl Default for InputGridConfig {
    fn default() -> Self {
        Self::new(GridSize::new(3, 3))
    }
}

/// Detected screen region for visualization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedScreenRegion {
    /// Screen ID
    pub screen_id: u32,
    /// Normalized corners [TL, TR, BR, BL] in 0-1 UV space
    pub corners: [(f32, f32); 4],
    /// Center position (normalized)
    pub center: (f32, f32),
    /// Width in normalized coordinates
    pub width: f32,
    /// Height in normalized coordinates
    pub height: f32,
    /// Detected aspect ratio
    pub aspect_ratio: AspectRatio,
    /// Detected orientation
    pub orientation: Orientation,
}

/// Complete video matrix configuration combining input grid and mappings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoMatrixConfig {
    /// Input grid configuration
    pub input_grid: InputGridConfig,
    
    /// Output grid size (derived or explicit)
    pub output_grid: GridSize,
    
    /// Background color for unmapped areas (default: black)
    pub background_color: [f32; 4],
    
    /// Whether to auto-detect from AprilTags
    pub auto_detect: bool,
    
    /// Detected screen regions from auto-detection (for visualization)
    pub detected_screens: Vec<DetectedScreenRegion>,
}

impl VideoMatrixConfig {
    /// Create a new video matrix config
    /// Output grid defaults to match input grid size (e.g., 3x3 input -> 3x3 output)
    pub fn new(input_grid_size: GridSize) -> Self {
        let input_grid = InputGridConfig::new(input_grid_size);
        // Default output grid to match input grid - user can change via GUI
        let output_grid = input_grid_size;
        
        Self {
            input_grid,
            output_grid,
            background_color: [0.0, 0.0, 0.0, 1.0], // Black
            auto_detect: true,
            detected_screens: Vec::new(),
        }
    }

    /// Set output grid size explicitly
    pub fn with_output_grid(mut self, grid: GridSize) -> Self {
        self.output_grid = grid;
        self
    }

    /// Set background color
    pub fn with_background_color(mut self, color: [f32; 4]) -> Self {
        self.background_color = color;
        self
    }

    /// Update output grid based on current mappings
    pub fn update_output_grid(&mut self) {
        self.output_grid = self.input_grid.calculate_output_grid_size();
    }

    /// Get all active mappings
    pub fn active_mappings(&self) -> Vec<&GridCellMapping> {
        self.input_grid.mappings.iter().filter(|m| m.enabled).collect()
    }

    /// Get mapping for a specific output position (if any)
    pub fn get_mapping_at_output(&self, col: f32, row: f32) -> Option<&GridCellMapping> {
        self.input_grid.mappings.iter().find(|m| {
            m.enabled &&
            col >= m.output_position.col &&
            col < m.output_position.col + m.output_position.width &&
            row >= m.output_position.row &&
            row < m.output_position.row + m.output_position.height
        })
    }
}

impl Default for VideoMatrixConfig {
    fn default() -> Self {
        Self::new(GridSize::new(3, 3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aspect_ratio() {
        assert!((AspectRatio::Ratio4_3.as_f32() - 1.333).abs() < 0.01);
        assert!((AspectRatio::Ratio16_9.as_f32() - 1.778).abs() < 0.01);
        assert_eq!(AspectRatio::Ratio1_1.as_f32(), 1.0);
    }

    #[test]
    fn test_aspect_ratio_detect() {
        assert_eq!(AspectRatio::detect(1920.0, 1080.0), AspectRatio::Ratio16_9);
        assert_eq!(AspectRatio::detect(1024.0, 768.0), AspectRatio::Ratio4_3);
        assert_eq!(AspectRatio::detect(100.0, 100.0), AspectRatio::Ratio1_1);
    }

    #[test]
    fn test_orientation() {
        assert_eq!(Orientation::Normal.degrees(), 0);
        assert_eq!(Orientation::Rotated90.degrees(), 90);
        assert_eq!(Orientation::Rotated180.degrees(), 180);
        assert_eq!(Orientation::Rotated270.degrees(), 270);
    }

    #[test]
    fn test_orientation_apply_uv() {
        let uv = Vec2::new(0.0, 0.0); // Top-left
        
        // Normal: stays top-left
        assert_eq!(Orientation::Normal.apply_to_uv(uv), Vec2::new(0.0, 0.0));
        
        // 90° CW: top-left becomes top-right (1, 0)
        assert_eq!(Orientation::Rotated90.apply_to_uv(uv), Vec2::new(1.0, 0.0));
        
        // 180°: becomes bottom-right
        assert_eq!(Orientation::Rotated180.apply_to_uv(uv), Vec2::new(1.0, 1.0));
        
        // 270°: becomes bottom-left
        assert_eq!(Orientation::Rotated270.apply_to_uv(uv), Vec2::new(0.0, 1.0));
    }

    #[test]
    fn test_input_grid_config() {
        let mut config = InputGridConfig::new(GridSize::new(3, 3));
        
        assert_eq!(config.total_cells(), 9);
        assert_eq!(config.cell_position(0), (0, 0));
        assert_eq!(config.cell_position(1), (1, 0));
        assert_eq!(config.cell_position(3), (0, 1));
        assert_eq!(config.cell_index(1, 1), 4);
        
        // Add a mapping
        let mapping = GridCellMapping::new(0, GridPosition::new(0.0, 0.0, 1.0, 1.0));
        config.add_mapping(mapping);
        
        assert!(config.is_cell_mapped(0));
        assert!(!config.is_cell_mapped(1));
        
        let unmapped = config.unmapped_cells();
        assert_eq!(unmapped.len(), 8);
        assert!(!unmapped.contains(&0));
    }

    #[test]
    fn test_grid_cell_mapping_source_rect() {
        let mapping = GridCellMapping::new(0, GridPosition::new(0.0, 0.0, 1.0, 1.0));
        let grid = GridSize::new(3, 3);
        
        let rect = mapping.get_source_rect(grid);
        assert!((rect.x - 0.0).abs() < 0.01);
        assert!((rect.y - 0.0).abs() < 0.01);
        assert!((rect.width - 0.333).abs() < 0.01);
        assert!((rect.height - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_video_matrix_config() {
        let mut config = VideoMatrixConfig::new(GridSize::new(2, 2));
        
        // Initially no mappings, so output grid should be 1x1
        assert_eq!(config.output_grid.columns, 1);
        assert_eq!(config.output_grid.rows, 1);
        
        // Add mappings
        config.input_grid.create_default_mapping();
        config.update_output_grid();
        
        assert_eq!(config.output_grid.columns, 2);
        assert_eq!(config.output_grid.rows, 2);
        
        assert_eq!(config.active_mappings().len(), 4);
    }
}
