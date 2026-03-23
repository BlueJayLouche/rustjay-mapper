//! # AprilTag Auto-Detection for Video Matrix
//!
//! Automatically detects screens in the input image using AprilTag markers.
//! Calculates screen regions, aspect ratios, and orientations from marker positions.
//!
//! ## Detection Strategy
//!
//! 1. Display AprilTags on each screen (centered or corner)
//! 2. Detect tags in the input image
//! 3. For each tag, calculate the screen region based on:
//!    - Tag center position
//!    - Known screen aspect ratio (from config or auto-detected)
//!    - Tag size relative to screen
//! 4. Determine orientation from tag rotation
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use rusty_mapper::videowall::{AprilTagAutoDetector, VideoMatrixConfig};
//!
//! // Detect screens from input image
//! let detector = AprilTagAutoDetector::new();
//! let detections = detector.detect_screens(&input_image, 2)?; // Expect 2 screens
//!
//! // Create video matrix config from detections
//! let config = detector.create_matrix_config(&detections, (1920, 1080))?;
//! ```

use super::{
    AprilTagDetection, AprilTagDetector, AprilTagFamily, AspectRatio, GridCellMapping,
    GridPosition, GridSize, InputGridConfig, Orientation, VideoMatrixConfig,
};
use glam::Vec2;
use image::GrayImage;

/// Tag placement strategy on screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagPlacement {
    /// Tag is centered on screen (default for test patterns)
    Centered,
    /// Tag is at top-left corner of screen
    TopLeft,
    /// Tag is at top-right corner of screen
    TopRight,
    /// Tag is at bottom-left corner of screen
    BottomLeft,
    /// Tag is at bottom-right corner of screen
    BottomRight,
}

impl Default for TagPlacement {
    fn default() -> Self {
        Self::Centered
    }
}

/// Configuration for AprilTag auto-detection
#[derive(Debug, Clone)]
pub struct AutoDetectConfig {
    /// Expected number of screens (for validation)
    pub expected_screens: usize,
    /// Tag family to use (default: Tag36h11)
    pub tag_family: AprilTagFamily,
    /// Physical size of tag relative to screen (e.g., 0.25 = 25% of screen width)
    pub tag_size_ratio: f32,
    /// Aspect ratio to assume when auto-detecting fails
    pub default_aspect_ratio: AspectRatio,
    /// Padding around detected region (as ratio of screen size)
    pub region_padding: f32,
    /// Minimum detection confidence
    pub min_confidence: f32,
    /// Where the tag is placed on the physical screen
    pub tag_placement: TagPlacement,
    /// Aspect ratio of the live input texture (width / height).
    /// Used to correctly scale source_rect HEIGHT in input UV space.
    /// Must match the internal resolution (e.g., 1920/1080 = 16/9).
    /// This is NOT the detection photo aspect — the photo aspect is only used
    /// for classifying the physical screen's aspect ratio from tag distortion.
    pub input_aspect: f32,
    /// After detection, uniformly scale and centre all source_rects so their
    /// bounding box fills the input frame edge-to-edge.  Relative positions
    /// and per-screen aspect ratios are preserved.  Maximises sampled pixels.
    pub fit_to_frame: bool,
}

impl Default for AutoDetectConfig {
    fn default() -> Self {
        Self {
            expected_screens: 2,
            tag_family: AprilTagFamily::Tag36h11,
            tag_size_ratio: 0.60,
            default_aspect_ratio: AspectRatio::Ratio16_9,
            region_padding: 0.0,
            min_confidence: 10.0,
            tag_placement: TagPlacement::Centered,
            input_aspect: 16.0 / 9.0,
            fit_to_frame: true,
        }
    }
}

/// Detected screen information from AprilTag
#[derive(Debug, Clone)]
pub struct DetectedScreen {
    /// Screen ID (from AprilTag ID)
    pub screen_id: u32,
    /// Normalized corners of the screen region [TL, TR, BR, BL] in 0-1 UV space
    pub corners: [Vec2; 4],
    /// Center position in normalized coordinates
    pub center: Vec2,
    /// Detected aspect ratio
    pub aspect_ratio: AspectRatio,
    /// Detected orientation
    pub orientation: Orientation,
    /// Raw AprilTag detection data
    pub tag_detection: AprilTagDetection,
    /// Screen width in normalized coordinates (0-1)
    pub width: f32,
    /// Screen height in normalized coordinates (0-1)
    pub height: f32,
}

impl DetectedScreen {
    /// Get source rectangle in normalized UV coordinates
    pub fn source_rect(&self) -> (f32, f32, f32, f32) {
        (self.corners[0].x, self.corners[0].y, self.width, self.height)
    }

    /// Check if a point is inside this screen region
    pub fn contains(&self, uv: Vec2) -> bool {
        uv.x >= self.corners[0].x
            && uv.x <= self.corners[2].x
            && uv.y >= self.corners[0].y
            && uv.y <= self.corners[2].y
    }
}

/// AprilTag auto-detector for video matrix screens
pub struct AprilTagAutoDetector {
    config: AutoDetectConfig,
}

impl AprilTagAutoDetector {
    /// Create a new auto-detector with default config
    pub fn new() -> Self {
        Self {
            config: AutoDetectConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: AutoDetectConfig) -> Self {
        Self { config }
    }

    /// Detect screens in an image
    ///
    /// # Arguments
    /// * `image` - Input grayscale image
    /// * `image_size` - (width, height) of the image for normalization
    ///
    /// # Returns
    /// Vector of detected screens, sorted by screen_id
    pub fn detect_screens(
        &self,
        image: &GrayImage,
        image_size: (u32, u32),
    ) -> anyhow::Result<Vec<DetectedScreen>> {
        let (img_width, img_height) = image_size;
        let img_width_f = img_width as f32;
        let img_height_f = img_height as f32;

        // Detect AprilTags
        let mut detector = AprilTagDetector::new(self.config.tag_family);
        let detections = detector.detect(image);

        log::info!(
            "AprilTag detection found {} markers (expected {})",
            detections.len(),
            self.config.expected_screens
        );

        // Filter by confidence and convert to screens
        let mut screens: Vec<DetectedScreen> = detections
            .into_iter()
            .filter(|d| d.decision_margin >= self.config.min_confidence)
            .map(|detection| self.detection_to_screen(&detection, img_width_f, img_height_f))
            .collect();

        // Sort by screen_id for consistent ordering
        screens.sort_by_key(|s| s.screen_id);

        log::info!(
            "Detected {} screens with sufficient confidence",
            screens.len()
        );

        // Log detection details before fit
        for screen in &screens {
            log::info!(
                "Screen {}: {:?} {:?} at ({:.3}, {:.3}), size {:.3}x{:.3}",
                screen.screen_id,
                screen.aspect_ratio.name(),
                screen.orientation,
                screen.center.x,
                screen.center.y,
                screen.width,
                screen.height
            );
        }

        // Scale all screens so their bounding box fills the input frame
        if self.config.fit_to_frame && screens.len() > 0 {
            Self::fit_screens_to_frame(&mut screens);
            log::info!("fit_to_frame applied — screens after scaling:");
            for screen in &screens {
                log::info!(
                    "  Screen {}: at ({:.3}, {:.3}), size {:.3}x{:.3}",
                    screen.screen_id, screen.center.x, screen.center.y,
                    screen.width, screen.height
                );
            }
        }

        Ok(screens)
    }

    /// Uniformly scale and centre all detected screens so that the bounding box
    /// of the group fills the input texture edge-to-edge in the tightest axis.
    ///
    /// Relative positions and per-screen aspect ratios are preserved.
    fn fit_screens_to_frame(screens: &mut Vec<DetectedScreen>) {
        // Bounding box of all screen corners
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for s in screens.iter() {
            for c in &s.corners {
                min_x = min_x.min(c.x);
                min_y = min_y.min(c.y);
                max_x = max_x.max(c.x);
                max_y = max_y.max(c.y);
            }
        }

        let bbox_w = max_x - min_x;
        let bbox_h = max_y - min_y;
        if bbox_w <= 0.0 || bbox_h <= 0.0 {
            return;
        }

        // Uniform scale: expand until the larger bounding-box axis reaches 1.0
        let scale = (1.0_f32 / bbox_w).min(1.0 / bbox_h);

        // Pivot from the bounding-box centre → map to UV centre (0.5, 0.5)
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;

        let tx = |v: f32| (v - cx) * scale + 0.5;
        let ty = |v: f32| (v - cy) * scale + 0.5;

        for s in screens.iter_mut() {
            s.corners = [
                Vec2::new(tx(s.corners[0].x), ty(s.corners[0].y)),
                Vec2::new(tx(s.corners[1].x), ty(s.corners[1].y)),
                Vec2::new(tx(s.corners[2].x), ty(s.corners[2].y)),
                Vec2::new(tx(s.corners[3].x), ty(s.corners[3].y)),
            ];
            s.center = Vec2::new(tx(s.center.x), ty(s.center.y));
            s.width  *= scale;
            s.height *= scale;
        }

        log::info!(
            "fit_to_frame: bbox ({:.3},{:.3})→({:.3},{:.3}) scale={:.3}",
            min_x, min_y, max_x, max_y, scale
        );
    }

    /// Convert AprilTag detection to screen region
    fn detection_to_screen(
        &self,
        detection: &AprilTagDetection,
        img_width: f32,
        img_height: f32,
    ) -> DetectedScreen {
        // Normalize corners to 0-1 UV space
        let corners: [Vec2; 4] = [
            Vec2::new(detection.corners[0][0] / img_width, detection.corners[0][1] / img_height),
            Vec2::new(detection.corners[1][0] / img_width, detection.corners[1][1] / img_height),
            Vec2::new(detection.corners[2][0] / img_width, detection.corners[2][1] / img_height),
            Vec2::new(detection.corners[3][0] / img_width, detection.corners[3][1] / img_height),
        ];

        let center = Vec2::new(
            detection.center[0] / img_width,
            detection.center[1] / img_height,
        );

        // Detect orientation from tag rotation
        let orientation = Orientation::detect_from_corners(&detection.corners);
        
        // Debug: log corner positions (AprilTag order: LB, RB, RT, LT)
        log::info!("Tag {} corners: [0]=({:.0},{:.0}), [1]=({:.0},{:.0}), [2]=({:.0},{:.0}), [3]=({:.0},{:.0})",
            detection.id,
            detection.corners[0][0], detection.corners[0][1],
            detection.corners[1][0], detection.corners[1][1],
            detection.corners[2][0], detection.corners[2][1],
            detection.corners[3][0], detection.corners[3][1]);

        // Image aspect ratio (width/height in pixels)
        let img_aspect = img_width / img_height;

        // Detect aspect ratio from tag distortion.
        // 
        // The tag on screen has been stretched from square to 16:9 (content aspect).
        // Then the screen's physical aspect ratio further distorts it:
        //   - 16:9 screen: no extra distortion (tag remains 16:9)
        //   - 4:3 screen: 16:9 content squashed horizontally → tag appears 4:3
        //   - 21:9 screen: 16:9 content stretched horizontally → tag appears 21:9
        //
        // If the screen is rotated 90°/270°, these distortions are rotated too.
        //
        // APPROACH:
        // 1. Detect orientation first (from tag rotation)
        // 2. Calculate the tag's apparent aspect ratio in UV space
        // 3. "Un-rotate" if needed to get the screen's native aspect ratio
        // 4. Classify based on the normalized (unrotated) ratio
        
        let tag_aspect_uv = self.calculate_tag_aspect_ratio(&corners);
        
        // Aspect ratio detection accounting for screen rotation.
        //
        // The tag is SQUARE on the physical display (fills height of 16:9 cell).
        // Screen aspect ratio distorts the 16:9 content, which distorts the tag:
        //   - 4:3 screen:  16:9 content squashed HORIZONTALLY → tag appears TALL (ratio ~0.75)
        //   - 16:9 screen: 16:9 content fits perfectly      → tag appears SQUARE (ratio ~1.0)
        //   - 21:9 screen: 16:9 content stretched HORIZONTALLY → tag appears WIDE (ratio ~1.31)
        //
        // When the SCREEN is rotated 90°/270°:
        //   - The tag rotates too
        //   - A TALL tag (4:3 screen) becomes WIDE in camera view
        //   - A WIDE tag (21:9 screen) becomes TALL in camera view
        //   - A SQUARE tag (16:9 screen) stays SQUARE
        //
        // So for rotated screens, we INVERT the ratio to get the true aspect.
        
        let is_rotated = matches!(orientation, Orientation::Rotated90 | Orientation::Rotated270);
        let normalized_aspect = if is_rotated {
            // Invert to "un-rotate" the measurement
            1.0 / tag_aspect_uv
        } else {
            tag_aspect_uv
        };
        
        let detected_aspect = self.detect_aspect_ratio_from_tag_aspect(normalized_aspect);
        
        // Debug logging
        let tag_width_pixels = (detection.corners[1][0] - detection.corners[0][0]).abs();
        let tag_height_pixels = (detection.corners[3][1] - detection.corners[0][1]).abs();
        let raw_pixel_ratio = if tag_height_pixels > 0.0 { 
            tag_width_pixels / tag_height_pixels 
        } else { 
            1.0 
        };
        log::info!("Tag {}: raw={:.3}, uv={:.3}, normalized={:.3}, rotated={}, -> {:?}",
            detection.id, raw_pixel_ratio, tag_aspect_uv, normalized_aspect, is_rotated, detected_aspect.name());
        
        // Calculate screen dimensions and corners based on placement.
        // NOTE: pass self.config.input_aspect (live input texture aspect) NOT img_aspect
        // (detection photo aspect). The source_rect height must be correct in INPUT UV
        // space, and the input texture is typically 16:9 regardless of what camera took
        // the detection photo.
        let input_aspect = self.config.input_aspect;
        let (screen_width, screen_height, screen_corners) = match self.config.tag_placement {
            TagPlacement::Centered => {
                self.calculate_centered_screen_with_aspect(&corners, center, detected_aspect, input_aspect)
            }
            TagPlacement::TopLeft => {
                self.calculate_corner_screen_with_aspect(&corners, center, orientation, detected_aspect, input_aspect)
            }
            _ => {
                self.calculate_centered_screen_with_aspect(&corners, center, detected_aspect, input_aspect)
            }
        };
        
        // Calculate final pixel-based aspect ratio for verification
        let pixel_width = screen_width * img_width;
        let pixel_height = screen_height * img_height;
        let calculated_ratio = screen_width / screen_height;
        let expected_ratio = detected_aspect.as_f32();
        
        let marker_to_fiducial = 0.8;
        log::info!("Screen {}: aspect={:?}, size={:.0}x{:.0}px (fiducial={:.0}px, slider={:.0}%, actual_fill={:.1}%)",
            detection.id, detected_aspect.name(), pixel_width, pixel_height, 
            tag_height_pixels, self.config.tag_size_ratio * 100.0, 
            self.config.tag_size_ratio * marker_to_fiducial * 100.0);

        DetectedScreen {
            screen_id: detection.id,
            corners: screen_corners,
            center,
            aspect_ratio: detected_aspect,
            orientation,
            tag_detection: detection.clone(),
            width: screen_width,
            height: screen_height,
        }
    }
    
    /// Detect screen aspect ratio from the normalized (unrotated) UV aspect ratio.
    ///
    /// The tag content is 16:9 (stretched from square to fill 16:9 frame).
    /// The screen's physical aspect ratio distorts this 16:9 content:
    ///   - 4:3  screen: 16:9 content squashed horizontally → ratio ≈ 0.75
    ///   - 16:9 screen: 16:9 content fits perfectly      → ratio ≈ 1.0
    ///   - 21:9 screen: 16:9 content stretched horizontally → ratio ≈ 1.31
    ///
    /// Note: This function receives the *normalized* ratio where rotation has already
    /// been accounted for (swapped back if the screen was rotated 90°/270°).
    ///
    /// Thresholds are midpoints between adjacent expected values (±15% perspective slack).
    fn detect_aspect_ratio_from_tag_aspect(&self, normalized_aspect: f32) -> AspectRatio {
        log::info!("Screen native aspect ratio detection: {:.3}", normalized_aspect);

        if normalized_aspect < 0.875 {
            log::info!("  -> Detected as 4:3 (ratio {:.3} < 0.875)", normalized_aspect);
            AspectRatio::Ratio4_3
        } else if normalized_aspect < 1.156 {
            log::info!("  -> Detected as 16:9 (ratio {:.3} in [0.875, 1.156))", normalized_aspect);
            AspectRatio::Ratio16_9
        } else {
            log::info!("  -> Detected as 21:9 (ratio {:.3} >= 1.156)", normalized_aspect);
            AspectRatio::Ratio21_9
        }
    }
    
    /// Calculate centered screen dimensions from the detected tag.
    ///
    /// Tags are square in the output 3×3 grid (640×360 cells), so at 100% marker size
    /// the tag fills 100% of the output cell HEIGHT (360px) and 56.25% of WIDTH.
    /// Therefore `tag_size_ratio` = fraction of **screen HEIGHT** the marker occupies.
    /// We measure fiducial HEIGHT and derive screen width from the known screen aspect.
    ///
    /// `img_aspect` here is the **input texture aspect** (e.g. 16/9), passed from
    /// `self.config.input_aspect` — NOT the detection photo aspect.
    fn calculate_centered_screen_with_aspect(
        &self,
        tag_corners: &[Vec2; 4],
        tag_center: Vec2,
        aspect_ratio: AspectRatio,
        img_aspect: f32, // = input_aspect from config
    ) -> (f32, f32, [Vec2; 4]) {
        // Corners: [TL, TR, BR, BL]
        // Measure the fiducial's VERTICAL extent (left and right edges).
        // tag_size_ratio = marker_height / screen_height  →  use height to recover height.
        let left_height  = (tag_corners[3] - tag_corners[0]).length(); // |BL - TL|
        let right_height = (tag_corners[2] - tag_corners[1]).length(); // |BR - TR|
        let fiducial_height_uv = (left_height + right_height) / 2.0;

        // fiducial = 0.8 × marker,  marker = tag_size_ratio × screen_height
        // → screen_height_uv = fiducial_height_uv / (0.8 × tag_size_ratio)
        let marker_to_fiducial_ratio = 0.8_f32;
        let actual_fill_h = self.config.tag_size_ratio * marker_to_fiducial_ratio;
        let screen_height_uv = fiducial_height_uv / actual_fill_h.clamp(0.1, 1.0);

        // Derive screen width from the known screen aspect and the input texture aspect.
        // In UV space:  screen_w_UV / screen_h_UV = screen_aspect / input_aspect
        let screen_aspect = aspect_ratio.as_f32();
        let screen_width_uv = screen_height_uv * screen_aspect / img_aspect;

        let half_width  = screen_width_uv  / 2.0;
        let half_height = screen_height_uv / 2.0;

        let screen_corners = [
            Vec2::new(tag_center.x - half_width, tag_center.y - half_height), // TL
            Vec2::new(tag_center.x + half_width, tag_center.y - half_height), // TR
            Vec2::new(tag_center.x + half_width, tag_center.y + half_height), // BR
            Vec2::new(tag_center.x - half_width, tag_center.y + half_height), // BL
        ];

        log::info!("Screen calc: aspect={:?}, input_aspect={:.3}, height_uv={:.3}, width_uv={:.3}",
            aspect_ratio.name(), img_aspect, screen_height_uv, screen_width_uv);

        (screen_width_uv, screen_height_uv, screen_corners)
    }
    
    /// Calculate corner-placed screen dimensions from the detected tag.
    ///
    /// Same axis convention as `calculate_centered_screen_with_aspect`:
    /// recover screen WIDTH from the horizontal fiducial measurement, then
    /// derive height from the screen aspect ratio.
    fn calculate_corner_screen_with_aspect(
        &self,
        tag_corners: &[Vec2; 4],
        _tag_center: Vec2,
        orientation: Orientation,
        aspect_ratio: AspectRatio,
        img_aspect: f32,
    ) -> (f32, f32, [Vec2; 4]) {
        // Vertical fiducial extent → screen height (tag fills 100% of screen height)
        let fiducial_height = (tag_corners[3] - tag_corners[0]).length(); // |BL - TL|

        let marker_to_fiducial_ratio = 0.8_f32;
        let actual_fill_h = self.config.tag_size_ratio * marker_to_fiducial_ratio;
        let screen_height_uv = fiducial_height / actual_fill_h.clamp(0.1, 1.0);
        let screen_aspect = aspect_ratio.as_f32();
        let screen_width_uv = screen_height_uv * screen_aspect / img_aspect;

        // Calculate screen corners based on tag placement (top-left)
        let tag_tl = tag_corners[0];
        let tag_tr = tag_corners[1];
        let tag_bl = tag_corners[3];

        // Calculate screen edges based on tag orientation
        let top_edge = tag_tr - tag_tl;
        let left_edge = tag_bl - tag_tl;

        // Normalize edge directions
        let top_dir = if top_edge.length() > 0.0 {
            top_edge.normalize()
        } else {
            Vec2::new(1.0, 0.0)
        };
        let left_dir = if left_edge.length() > 0.0 {
            left_edge.normalize()
        } else {
            Vec2::new(0.0, 1.0)
        };

        let screen_tl = tag_tl;
        let screen_tr = tag_tl + top_dir * screen_width_uv;
        let screen_bl = tag_tl + left_dir * screen_height_uv;
        let screen_br = screen_bl + top_dir * screen_width_uv;

        let screen_corners = [screen_tl, screen_tr, screen_br, screen_bl];

        // Apply orientation swap if needed
        match orientation {
            Orientation::Rotated90 | Orientation::Rotated270 => {
                (screen_height_uv, screen_width_uv, screen_corners)
            }
            _ => (screen_width_uv, screen_height_uv, screen_corners),
        }
    }
    
    /// Calculate tag aspect ratio from detected corners
    /// Returns width/height ratio (1.0 = square, <1 = squeezed horizontally, >1 = stretched horizontally)
    fn calculate_tag_aspect_ratio(&self, corners: &[Vec2; 4]) -> f32 {
        // Calculate tag width (average of top and bottom edges)
        let top_width = (corners[1] - corners[0]).length();
        let bottom_width = (corners[2] - corners[3]).length();
        let tag_width = (top_width + bottom_width) / 2.0;
        
        // Calculate tag height (average of left and right edges)
        let left_height = (corners[3] - corners[0]).length();
        let right_height = (corners[2] - corners[1]).length();
        let tag_height = (left_height + right_height) / 2.0;
        
        if tag_height > 0.0 {
            tag_width / tag_height
        } else {
            1.0 // Assume square if can't calculate
        }
    }

    /// Calculate screen region when tag is centered on screen
    ///
    /// The tag is displayed in the center of the screen as a test pattern.
    /// We calculate screen bounds by extending from the tag based on the
    /// known tag-to-screen size ratio.
    fn calculate_centered_screen(
        &self,
        tag_corners: &[Vec2; 4],
        tag_center: Vec2,
        img_width: f32,
        img_height: f32,
    ) -> (f32, f32, [Vec2; 4]) {
        // Calculate tag width/height in normalized coordinates
        let tag_width = (tag_corners[1] - tag_corners[0]).length();
        let tag_height = (tag_corners[3] - tag_corners[0]).length();
        let tag_avg_size = (tag_width + tag_height) / 2.0;

        // Calculate screen size from tag ratio
        // tag_size_ratio = tag_width / screen_width
        let screen_width = tag_avg_size / self.config.tag_size_ratio;
        let screen_height = screen_width / self.config.default_aspect_ratio.as_f32();

        // Calculate screen corners centered on tag_center
        let half_width = screen_width / 2.0;
        let half_height = screen_height / 2.0;

        let screen_corners = [
            Vec2::new(tag_center.x - half_width, tag_center.y - half_height), // TL
            Vec2::new(tag_center.x + half_width, tag_center.y - half_height), // TR
            Vec2::new(tag_center.x + half_width, tag_center.y + half_height), // BR
            Vec2::new(tag_center.x - half_width, tag_center.y + half_height), // BL
        ];

        (screen_width, screen_height, screen_corners)
    }

    /// Calculate screen region when tag is at a corner
    fn calculate_corner_screen(
        &self,
        tag_corners: &[Vec2; 4],
        _tag_center: Vec2,
        orientation: Orientation,
        img_width: f32,
        img_height: f32,
    ) -> (f32, f32, [Vec2; 4]) {
        // Calculate actual tag width and height in pixels
        let tag_width = (tag_corners[1] - tag_corners[0]).length() * img_width;
        let tag_height = (tag_corners[3] - tag_corners[0]).length() * img_height;
        let tag_avg_size = (tag_width + tag_height) / 2.0;

        // Calculate expected screen size based on tag ratio
        let expected_screen_width = tag_avg_size / self.config.tag_size_ratio;
        let expected_screen_height =
            expected_screen_width / self.config.default_aspect_ratio.as_f32();

        // Normalize to 0-1
        let screen_width_norm = expected_screen_width / img_width;
        let screen_height_norm = expected_screen_height / img_height;

        // Calculate screen corners based on tag placement
        // For now, assume top-left placement
        let tag_tl = tag_corners[0];
        let tag_tr = tag_corners[1];
        let tag_bl = tag_corners[3];

        // Calculate screen edges based on tag orientation
        let top_edge = tag_tr - tag_tl;
        let left_edge = tag_bl - tag_tl;

        // Normalize edge directions
        let top_dir = if top_edge.length() > 0.0 {
            top_edge.normalize()
        } else {
            Vec2::new(1.0, 0.0)
        };
        let left_dir = if left_edge.length() > 0.0 {
            left_edge.normalize()
        } else {
            Vec2::new(0.0, 1.0)
        };

        let screen_tl = tag_tl;
        let screen_tr = tag_tl + top_dir * screen_width_norm;
        let screen_bl = tag_tl + left_dir * screen_height_norm;
        let screen_br = screen_bl + top_dir * screen_width_norm;

        let screen_corners = [screen_tl, screen_tr, screen_br, screen_bl];

        // Apply orientation swap if needed
        match orientation {
            Orientation::Rotated90 | Orientation::Rotated270 => {
                // Portrait orientation - swap dimensions
                (screen_height_norm, screen_width_norm, screen_corners)
            }
            _ => (screen_width_norm, screen_height_norm, screen_corners),
        }
    }

    /// Create video matrix config from detected screens
    ///
    /// Maps detected screens to specific output cells based on screen_id (AprilTag ID).
    /// Output position is calculated as: col = screen_id % output_cols, row = screen_id / output_cols
    /// Example for 3x3 grid: Screen ID 4 → (1, 1), Screen ID 7 → (1, 2)
    /// Remaining cells are left empty (will show black).
    ///
    /// # Arguments
    /// * `screens` - Detected screens from detect_screens()
    /// * `input_resolution` - (width, height) of input texture
    /// * `output_grid` - Output grid size (columns, rows)
    ///
    /// # Returns
    /// Configured VideoMatrixConfig with specified output grid
    pub fn create_matrix_config(
        &self,
        screens: &[DetectedScreen],
        _input_resolution: (u32, u32),
        output_grid: GridSize,
    ) -> anyhow::Result<VideoMatrixConfig> {
        if screens.is_empty() {
            anyhow::bail!("No screens detected");
        }

        let input_grid_size = GridSize::new(screens.len() as u32, 1);
        let max_screen_id = output_grid.columns * output_grid.rows;
        
        // Create input grid config
        let mut input_grid = InputGridConfig::new(input_grid_size);

        // Create mapping for each detected screen based on its screen_id (AprilTag ID)
        for (idx, screen) in screens.iter().enumerate() {
            let screen_id = screen.screen_id;
            let output_col = (screen_id % output_grid.columns) as f32;
            let output_row = (screen_id / output_grid.columns) as f32;
            
            // Ensure we don't go out of bounds
            if screen_id >= max_screen_id {
                log::warn!(
                    "Screen ID {} exceeds output grid bounds (max {} for {}x{} grid), skipping",
                    screen_id, max_screen_id - 1, output_grid.columns, output_grid.rows
                );
                continue;
            }
            
            let output_position = GridPosition::new(output_col, output_row, 1.0, 1.0);

            log::info!(
                "Mapping detected screen {} (ID {}) → output position ({}, {})",
                idx, screen_id, output_col, output_row
            );

            // Create source rect from detected screen corners (normalized UV coordinates)
            let source_rect = super::Rect::new(
                screen.corners[0].x, // Top-left X
                screen.corners[0].y, // Top-left Y
                screen.width,        // Width
                screen.height,       // Height
            );

            let mapping = GridCellMapping::new(idx, output_position)
                .with_aspect_ratio(screen.aspect_ratio)
                .with_orientation(screen.orientation)
                .with_display_id(screen_id)
                .with_source_rect(source_rect);

            input_grid.add_mapping(mapping);
        }

        Ok(VideoMatrixConfig {
            input_grid,
            output_grid,
            background_color: [0.0, 0.0, 0.0, 1.0],
            auto_detect: true,
            detected_screens: Vec::new(),
        })
    }

    /// Create video matrix config with specific output grid (convenience method)
    pub fn create_matrix_config_with_grid(
        &self,
        screens: &[DetectedScreen],
        input_resolution: (u32, u32),
        output_grid: GridSize,
    ) -> anyhow::Result<VideoMatrixConfig> {
        self.create_matrix_config(screens, input_resolution, output_grid)
    }

    /// Create a simple 2-screen side-by-side configuration
    ///
    /// This is a convenience method for the common case of 2 screens
    pub fn create_two_screen_config(
        &self,
        screen0_aspect: AspectRatio,
        screen1_aspect: AspectRatio,
    ) -> VideoMatrixConfig {
        let grid_size = GridSize::new(2, 1);
        let mut input_grid = InputGridConfig::new(grid_size);

        // Screen 0 (left)
        let mapping_0 = GridCellMapping::new(0, GridPosition::new(0.0, 0.0, 1.0, 1.0))
            .with_aspect_ratio(screen0_aspect)
            .with_display_id(0);

        // Screen 1 (right)
        let mapping_1 = GridCellMapping::new(1, GridPosition::new(1.0, 0.0, 1.0, 1.0))
            .with_aspect_ratio(screen1_aspect)
            .with_display_id(1);

        input_grid.add_mapping(mapping_0);
        input_grid.add_mapping(mapping_1);

        VideoMatrixConfig {
            input_grid,
            output_grid: GridSize::new(2, 1),
            background_color: [0.0, 0.0, 0.0, 1.0],
            auto_detect: false,
            detected_screens: Vec::new(),
        }
    }

    /// Quick detect and configure in one step
    ///
    /// Uses default 3x3 output grid. Use `create_matrix_config` directly for custom grid sizes.
    ///
    /// # Arguments
    /// * `image` - Input grayscale image
    /// * `image_size` - (width, height) of input image
    /// * `input_resolution` - Resolution for input texture reference
    pub fn auto_configure(
        &self,
        image: &GrayImage,
        image_size: (u32, u32),
        input_resolution: (u32, u32),
    ) -> anyhow::Result<VideoMatrixConfig> {
        let screens = self.detect_screens(image, image_size)?;

        if screens.len() != self.config.expected_screens {
            log::warn!(
                "Detected {} screens but expected {}",
                screens.len(),
                self.config.expected_screens
            );
        }

        // Default to 3x3 grid for convenience
        self.create_matrix_config(&screens, input_resolution, GridSize::new(3, 3))
    }

    /// Get current config
    pub fn config(&self) -> &AutoDetectConfig {
        &self.config
    }

    /// Update config
    pub fn set_config(&mut self, config: AutoDetectConfig) {
        self.config = config;
    }
}

impl Default for AprilTagAutoDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to extract a grayscale image from a wgpu texture
/// For use when running detection on GPU textures
pub fn texture_to_gray_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> anyhow::Result<GrayImage> {
    // Create buffer to read texture data
    let buffer_size = (width * height * 4) as u64; // BGRA8
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("AprilTag Readback Buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Copy texture to buffer
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("AprilTag Copy Encoder"),
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    queue.submit(std::iter::once(encoder.finish()));

    // Map buffer and convert to grayscale
    let buffer_slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });

    // Poll device to process buffer mapping
    device.poll(wgpu::PollType::Wait)?;

    // Wait for mapping to complete
    receiver.recv()?.map_err(|e| anyhow::anyhow!("Buffer mapping failed: {:?}", e))?;

    let data = buffer_slice.get_mapped_range();
    let mut gray_data = Vec::with_capacity((width * height) as usize);

    // Convert BGRA to grayscale (luminance)
    for chunk in data.chunks_exact(4) {
        let b = chunk[0] as f32;
        let g = chunk[1] as f32;
        let r = chunk[2] as f32;
        // ITU-R BT.601 luma coefficients
        let luma = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        gray_data.push(luma);
    }

    drop(data);
    buffer.unmap();

    GrayImage::from_raw(width, height, gray_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create grayscale image"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_config_default() {
        let config = AutoDetectConfig::default();
        assert_eq!(config.expected_screens, 2);
        assert_eq!(config.tag_size_ratio, 0.60);
        assert!(matches!(config.default_aspect_ratio, AspectRatio::Ratio16_9));
        assert!(matches!(config.tag_placement, TagPlacement::Centered));
    }

    #[test]
    fn test_tag_placement_variants() {
        assert!(matches!(TagPlacement::Centered, TagPlacement::Centered));
        assert!(matches!(TagPlacement::TopLeft, TagPlacement::TopLeft));
    }

    #[test]
    fn test_two_screen_config() {
        let detector = AprilTagAutoDetector::new();
        let config = detector.create_two_screen_config(
            AspectRatio::Ratio4_3,
            AspectRatio::Ratio16_9,
        );

        assert_eq!(config.output_grid.columns, 2);
        assert_eq!(config.output_grid.rows, 1);
        assert_eq!(config.input_grid.mappings.len(), 2);

        // Check first mapping
        let mapping0 = &config.input_grid.mappings[0];
        assert_eq!(mapping0.input_cell, 0);
        assert!(matches!(mapping0.aspect_ratio, AspectRatio::Ratio4_3));

        // Check second mapping
        let mapping1 = &config.input_grid.mappings[1];
        assert_eq!(mapping1.input_cell, 1);
        assert!(matches!(mapping1.aspect_ratio, AspectRatio::Ratio16_9));
    }
}
