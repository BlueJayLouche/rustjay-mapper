//! # AprilTag Marker Detection
//!
//! Pure Rust implementation of AprilTag fiducial marker detection.
//! Replaces the OpenCV-dependent ArUco detection.
//!
//! ## AprilTag Families
//!
//! - `tag36h11` - Recommended (36 bits, 11 minimum hamming distance)
//! - `tag25h9` - Smaller markers (25 bits, 9 hamming distance)
//! - `tag16h5` - Very small markers (16 bits, 5 hamming distance)
//!
//! ## Example
//!
//! ```rust,no_run
//! use rusty_mapper::videowall::{AprilTagDetector, AprilTagFamily};
//!
//! let mut detector = AprilTagDetector::new(AprilTagFamily::Tag36h11);
//! let image = image::open("tag.png").unwrap().to_luma8();
//! let detections = detector.detect(&image);
//!
//! for det in &detections {
//!     println!("Detected ID {} at {:?}", det.id, det.corners);
//! }
//! ```

use apriltag::{Detector, Family, Image as AprilTagImage};
use apriltag_sys;
use image::GrayImage;

/// AprilTag families available for detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AprilTagFamily {
    /// 36h11 - 36 bits, 11 hamming distance (recommended, 587 markers)
    Tag36h11,
    /// 25h9 - 25 bits, 9 hamming distance (587 markers)
    Tag25h9,
    /// 16h5 - 16 bits, 5 hamming distance (587 markers)
    Tag16h5,
}

impl AprilTagFamily {
    /// Get the family name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::Tag36h11 => "tag36h11",
            Self::Tag25h9 => "tag25h9",
            Self::Tag16h5 => "tag16h5",
        }
    }

    /// Get the filename prefix for this family
    pub fn filename_prefix(&self) -> &'static str {
        match self {
            Self::Tag36h11 => "tag36_11",
            Self::Tag25h9 => "tag25_09",
            Self::Tag16h5 => "tag16_05",
        }
    }

    /// Get the number of markers in this family
    pub fn marker_count(&self) -> u32 {
        // All these families have 587 markers
        587
    }

    /// Check if a marker ID is valid for this family
    pub fn is_valid_id(&self, id: u32) -> bool {
        id < self.marker_count()
    }

    /// Get the recommended family for a grid size
    pub fn for_grid_size(_columns: u32, _rows: u32) -> Self {
        // Always use Tag36h11 for best detection
        Self::Tag36h11
    }

    /// Convert to apriltag Family
    fn to_family(&self) -> Family {
        match self {
            Self::Tag36h11 => Family::tag_36h11(),
            Self::Tag25h9 => Family::tag_25h9(),
            Self::Tag16h5 => Family::tag_16h5(),
        }
    }
}

impl Default for AprilTagFamily {
    fn default() -> Self {
        Self::Tag36h11
    }
}

/// A detected AprilTag marker
#[derive(Debug, Clone)]
pub struct AprilTagDetection {
    /// Marker ID
    pub id: u32,
    /// Corner positions in image coordinates (top-left, top-right, bottom-right, bottom-left)
    pub corners: [[f32; 2]; 4],
    /// Center position
    pub center: [f32; 2],
    /// Detection confidence/decision margin
    pub decision_margin: f32,
    /// Hamming distance (0 = perfect match, higher = more errors corrected)
    pub hamming: u32,
}

/// AprilTag marker detector
pub struct AprilTagDetector {
    detector: Detector,
    family: AprilTagFamily,
}

impl AprilTagDetector {
    /// Create a new detector with the specified family
    pub fn new(family: AprilTagFamily) -> Self {
        let detector = Detector::builder()
            .add_family_bits(family.to_family(), 1)
            .build()
            .expect("Failed to create AprilTag detector");

        Self { detector, family }
    }

    /// Detect all markers in a grayscale image
    ///
    /// # Arguments
    /// * `image` - Grayscale image to search for markers
    ///
    /// # Returns
    /// Vector of detected markers
    pub fn detect(&mut self, image: &GrayImage) -> Vec<AprilTagDetection> {
        // Convert to AprilTag image format manually
        // (we can't use apriltag-image due to image crate version mismatch)
        let (width, height) = image.dimensions();
        let mut apriltag_image = AprilTagImage::zeros_with_alignment(
            width as usize,
            height as usize,
            96, // DEFAULT_ALIGNMENT_U8
        ).expect("Failed to create AprilTag image");

        // Copy pixel data
        for (x, y, pixel) in image.enumerate_pixels() {
            apriltag_image[(x as usize, y as usize)] = pixel[0];
        }

        // Detect markers
        let detections = self.detector.detect(&apriltag_image);

        // Convert to our format
        detections
            .into_iter()
            .map(|det| {
                let corners_array = det.corners();
                let mut corners = [[0.0f32; 2]; 4];
                for (i, corner) in corners_array.iter().enumerate().take(4) {
                    corners[i] = [corner[0] as f32, corner[1] as f32];
                }

                let center_array = det.center();

                AprilTagDetection {
                    id: det.id() as u32,
                    corners,
                    center: [center_array[0] as f32, center_array[1] as f32],
                    decision_margin: det.decision_margin(),
                    hamming: det.hamming() as u32,
                }
            })
            .collect()
    }

    /// Detect a specific marker ID
    pub fn detect_specific(&mut self, image: &GrayImage, target_id: u32) -> Option<AprilTagDetection> {
        self.detect(image)
            .into_iter()
            .find(|det| det.id == target_id)
    }

    /// Get the family used by this detector
    pub fn family(&self) -> AprilTagFamily {
        self.family
    }

    /// Set the number of threads used for detection
    pub fn set_thread_number(&mut self, nthreads: u8) {
        self.detector.set_thread_number(nthreads);
    }

    /// Set decimation (higher = faster but less accurate)
    ///
    /// - 1.0 = no decimation (full resolution)
    /// - 2.0 = half resolution
    /// - 4.0 = quarter resolution
    pub fn set_decimation(&mut self, decimate: f32) {
        self.detector.set_decimation(decimate);
    }

    /// Set Gaussian blur sigma (0.0 = no blur)
    pub fn set_sigma(&mut self, sigma: f32) {
        self.detector.set_sigma(sigma);
    }

    /// Enable/disable edge refinement
    pub fn set_refine_edges(&mut self, refine: bool) {
        self.detector.set_refine_edges(refine);
    }
}

impl Default for AprilTagDetector {
    fn default() -> Self {
        Self::new(AprilTagFamily::default())
    }
}

/// AprilTag marker generator
/// 
/// Note: AprilTags are typically pre-generated and stored as images.
/// This struct provides utilities for working with marker images.
#[derive(Debug)]
pub struct AprilTagGenerator {
    family: AprilTagFamily,
}

impl AprilTagGenerator {
    /// Create a new generator for the specified family
    pub fn new(family: AprilTagFamily) -> Self {
        Self { family }
    }

    /// Get the marker image filename for a given ID
    ///
    /// # Example
    /// ```
    /// use rusty_mapper::videowall::{AprilTagGenerator, AprilTagFamily};
    ///
    /// let gen = AprilTagGenerator::new(AprilTagFamily::Tag36h11);
    /// assert_eq!(gen.marker_filename(0), "tag36_11_00000.png");
    /// assert_eq!(gen.marker_filename(42), "tag36_11_00042.png");
    /// ```
    pub fn marker_filename(&self, id: u32) -> String {
        format!("{}_{:05}.png", self.family.filename_prefix(), id)
    }

    /// Get the path to a marker image in the assets directory
    pub fn marker_path(&self, id: u32) -> Option<std::path::PathBuf> {
        if !self.family.is_valid_id(id) {
            return None;
        }
        
        let filename = self.marker_filename(id);
        let path = std::path::PathBuf::from("assets/apriltags").join(filename);
        Some(path)
    }

    /// Load a marker image by ID from a pre-generated PNG file.
    ///
    /// Falls back to [`generate_marker`] if the file is not found.
    pub fn load_marker(&self, id: u32) -> anyhow::Result<GrayImage> {
        // Try loading from file first (for backward compat / custom markers)
        if let Some(path) = self.marker_path(id) {
            if path.exists() {
                let img = image::open(&path)?;
                return Ok(img.to_luma8());
            }
        }
        // Fall back to runtime generation
        self.generate_marker(id)
    }

    /// Generate a marker image at runtime using the apriltag C library.
    ///
    /// This works for any valid marker ID (0–586 for tag36h11) without
    /// needing pre-generated PNG files on disk.
    pub fn generate_marker(&self, id: u32) -> anyhow::Result<GrayImage> {
        if !self.family.is_valid_id(id) {
            anyhow::bail!("Invalid marker ID {} for family {}", id, self.family.name());
        }

        unsafe {
            // Create the C family struct
            let fam_ptr = match self.family {
                AprilTagFamily::Tag36h11 => apriltag_sys::tag36h11_create(),
                AprilTagFamily::Tag25h9  => apriltag_sys::tag25h9_create(),
                AprilTagFamily::Tag16h5  => apriltag_sys::tag16h5_create(),
            };
            if fam_ptr.is_null() {
                anyhow::bail!("Failed to create apriltag family");
            }

            let img_ptr = apriltag_sys::apriltag_to_image(fam_ptr, id as i32);
            if img_ptr.is_null() {
                apriltag_sys::tag36h11_destroy(fam_ptr);
                anyhow::bail!("apriltag_to_image returned null for id {}", id);
            }

            let img = &*img_ptr;
            let w = img.width as u32;
            let h = img.height as u32;
            let stride = img.stride as usize;

            let mut gray = GrayImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let val = *img.buf.add(y as usize * stride + x as usize);
                    gray.put_pixel(x, y, image::Luma([val]));
                }
            }

            apriltag_sys::image_u8_destroy(img_ptr);

            // Destroy using the correct family destructor
            match self.family {
                AprilTagFamily::Tag36h11 => apriltag_sys::tag36h11_destroy(fam_ptr),
                AprilTagFamily::Tag25h9  => apriltag_sys::tag25h9_destroy(fam_ptr),
                AprilTagFamily::Tag16h5  => apriltag_sys::tag16h5_destroy(fam_ptr),
            }

            Ok(gray)
        }
    }

    /// Get the family
    pub fn family(&self) -> AprilTagFamily {
        self.family
    }

    /// Generate a calibration frame with a single marker for a display
    ///
    /// Similar to ArUco's generate_calibration_frame
    pub fn generate_calibration_frame(
        &self,
        display_id: u32,
        grid_size: (u32, u32),
        output_resolution: (u32, u32),
        marker_size_percent: f32,
    ) -> anyhow::Result<image::RgbaImage> {
        let (cols, rows) = grid_size;
        let (width, height) = output_resolution;

        if !self.family.is_valid_id(display_id) {
            anyhow::bail!("Display ID {} exceeds family capacity", display_id);
        }

        // Calculate display region size
        let display_width = width / cols;
        let display_height = height / rows;

        // Calculate which row and column this display is in
        let col = display_id % cols;
        let row = display_id / cols;

        // Load the marker image
        let marker = self.load_marker(display_id)?;
        
        // Calculate marker size
        let target_size = (display_width.min(display_height) as f32 * marker_size_percent) as u32;
        
        // Resize marker to target size
        let resized = image::imageops::resize(
            &marker,
            target_size,
            target_size,
            image::imageops::FilterType::Nearest
        );

        // Create full-size black frame
        let mut frame = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));

        // Calculate marker position (centered in display region)
        let region_x = col * display_width;
        let region_y = row * display_height;
        let marker_x = region_x + (display_width - target_size) / 2;
        let marker_y = region_y + (display_height - target_size) / 2;

        // Copy marker into frame (convert grayscale to RGBA)
        for y in 0..target_size {
            for x in 0..target_size {
                let pixel = resized.get_pixel(x, y);
                let value = pixel[0];
                // White marker on black background
                let color = if value > 128 {
                    image::Rgba([255, 255, 255, 255])
                } else {
                    image::Rgba([0, 0, 0, 255])
                };
                frame.put_pixel(marker_x + x, marker_y + y, color);
            }
        }

        Ok(frame)
    }

    /// Generate a single frame with ALL markers displayed simultaneously
    ///
    /// Used for static pattern calibration where all displays show their markers at once
    pub fn generate_all_markers_frame(
        &self,
        grid_size: (u32, u32),
        output_resolution: (u32, u32),
        marker_size_percent: f32,
    ) -> anyhow::Result<image::RgbaImage> {
        let (cols, rows) = grid_size;
        let (width, height) = output_resolution;
        let total_displays = (cols * rows) as u32;

        // Validate all display IDs are valid for our family
        for id in 0..total_displays {
            if !self.family.is_valid_id(id) {
                anyhow::bail!(
                    "Display ID {} exceeds family capacity (max: {})",
                    id,
                    self.family.marker_count() - 1
                );
            }
        }

        // Calculate display region size
        let display_width = width / cols;
        let display_height = height / rows;

        // Create full-size black frame
        let mut frame = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));

        // Generate and place each marker
        for id in 0..total_displays {
            // Calculate which row and column this display is in
            let col = id % cols;
            let row = id / cols;

            // Calculate marker size
            let marker_size = ((display_width.min(display_height) as f32 * marker_size_percent) as u32)
                .max(50); // Minimum 50px

            // Load and resize the marker
            let marker = self.load_marker(id)?;
            let resized = image::imageops::resize(
                &marker,
                marker_size,
                marker_size,
                image::imageops::FilterType::Nearest
            );

            // Calculate marker position (centered in display region)
            let region_x = col * display_width;
            let region_y = row * display_height;
            let marker_x = region_x + (display_width - marker_size) / 2;
            let marker_y = region_y + (display_height - marker_size) / 2;

            // Copy marker into frame
            for y in 0..marker_size {
                for x in 0..marker_size {
                    let pixel = resized.get_pixel(x, y);
                    let value = pixel[0];
                    let color = if value > 128 {
                        image::Rgba([255, 255, 255, 255])
                    } else {
                        image::Rgba([0, 0, 0, 255])
                    };
                    frame.put_pixel(marker_x + x, marker_y + y, color);
                }
            }
        }

        Ok(frame)
    }

    /// Generate all calibration frames for a grid
    pub fn generate_all_calibration_frames(
        &self,
        grid_size: (u32, u32),
        output_resolution: (u32, u32),
        marker_size_percent: f32,
    ) -> anyhow::Result<Vec<image::RgbaImage>> {
        let (cols, rows) = grid_size;
        let total = (cols * rows) as u32;

        let mut frames = Vec::with_capacity(total as usize);
        for id in 0..total {
            let frame = self.generate_calibration_frame(
                id,
                grid_size,
                output_resolution,
                marker_size_percent
            )?;
            frames.push(frame);
        }

        Ok(frames)
    }
}

impl Default for AprilTagGenerator {
    fn default() -> Self {
        Self::new(AprilTagFamily::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_properties() {
        assert_eq!(AprilTagFamily::Tag36h11.marker_count(), 587);
        assert_eq!(AprilTagFamily::Tag36h11.name(), "tag36h11");
        assert!(AprilTagFamily::Tag36h11.is_valid_id(0));
        assert!(AprilTagFamily::Tag36h11.is_valid_id(586));
        assert!(!AprilTagFamily::Tag36h11.is_valid_id(587));
    }

    #[test]
    fn test_marker_filename() {
        let gen = AprilTagGenerator::new(AprilTagFamily::Tag36h11);
        assert_eq!(gen.marker_filename(0), "tag36_11_00000.png");
        assert_eq!(gen.marker_filename(42), "tag36_11_00042.png");
        assert_eq!(gen.marker_filename(586), "tag36_11_00586.png");
        
        let gen25 = AprilTagGenerator::new(AprilTagFamily::Tag25h9);
        assert_eq!(gen25.marker_filename(0), "tag25_09_00000.png");
        
        let gen16 = AprilTagGenerator::new(AprilTagFamily::Tag16h5);
        assert_eq!(gen16.marker_filename(0), "tag16_05_00000.png");
    }

    #[test]
    fn test_detector_creation() {
        let detector = AprilTagDetector::new(AprilTagFamily::Tag36h11);
        assert_eq!(detector.family(), AprilTagFamily::Tag36h11);
    }

    #[test]
    fn test_generate_marker() {
        let gen = AprilTagGenerator::new(AprilTagFamily::Tag36h11);

        // Generate marker 0
        let marker = gen.generate_marker(0).expect("Failed to generate marker 0");
        assert!(marker.width() > 0);
        assert!(marker.height() > 0);

        // Generate marker 15 (needed for 4x4 grids)
        let marker15 = gen.generate_marker(15).expect("Failed to generate marker 15");
        assert!(marker15.width() > 0);

        // Invalid ID should fail
        assert!(gen.generate_marker(587).is_err());
    }

    #[test]
    fn test_detect_generated_marker() {
        // Generate a marker at runtime and verify it can be detected
        let gen = AprilTagGenerator::new(AprilTagFamily::Tag36h11);
        let marker = gen.generate_marker(0).expect("Failed to generate marker");

        // Upscale for reliable detection
        let upscaled = image::imageops::resize(
            &marker,
            marker.width() * 20,
            marker.height() * 20,
            image::imageops::FilterType::Nearest,
        );

        let mut detector = AprilTagDetector::new(AprilTagFamily::Tag36h11);
        let detections = detector.detect(&upscaled);

        assert!(!detections.is_empty(), "Should detect the generated marker");
        assert_eq!(detections[0].id, 0, "Should detect ID 0");
    }

    #[test]
    fn test_family_for_grid_size() {
        // AprilTags always use Tag36h11 for best detection
        assert_eq!(AprilTagFamily::for_grid_size(2, 2), AprilTagFamily::Tag36h11);
        assert_eq!(AprilTagFamily::for_grid_size(5, 5), AprilTagFamily::Tag36h11);
        assert_eq!(AprilTagFamily::for_grid_size(10, 10), AprilTagFamily::Tag36h11);
    }

    #[test]
    fn test_invalid_marker_id() {
        let gen = AprilTagGenerator::new(AprilTagFamily::Tag36h11);
        assert!(!AprilTagFamily::Tag36h11.is_valid_id(1000));
        assert!(gen.marker_path(1000).is_none());
    }
}
