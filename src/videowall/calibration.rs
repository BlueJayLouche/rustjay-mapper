//! # Video Wall Calibration Controller
//!
//! Manages the calibration workflow for video wall auto-calibration using
//! static ArUco patterns. Supports both real-time camera capture and
//! photo/video upload modes.
//!
//! ## Static Pattern Calibration Flow
//!
//! ```text
//! Idle → Countdown → ShowingAllPatterns → Captured → Processing → Complete
//!                           │                   │
//!                           └───────────────────┘
//!                              (single frame)
//! ```
//!
//! All displays show their unique ArUco markers simultaneously. A single
//! camera frame captures all markers at once, enabling faster calibration
//! and support for photo-based processing.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use rusty_mapper::videowall::{
//!     CalibrationController, CalibrationMode, GridSize
//! };
//!
//! // Start real-time calibration
//! let mut cal = CalibrationController::new();
//! cal.start_realtime(GridSize::new(3, 3), (1920, 1080), (1920, 1080));
//!
//! // In your main loop
//! loop {
//!     match cal.update() {
//!         CalibrationStatus::InProgress => continue,
//!         CalibrationStatus::ReadyForCapture => {
//!             // Show "Click Capture" button
//!         }
//!         CalibrationStatus::Complete(config) => break,
//!         CalibrationStatus::Error(e) => handle_error(e),
//!     }
//! }
//! ```

use super::{ArUcoGenerator, ArUcoDictionary, DetectedMarker, DisplayQuad, GridSize, VideoWallConfig, CalibrationInfo, QuadMapper, QuadMapConfig};
use std::path::Path;
use std::time::Instant;

/// Calibration mode - real-time or from recorded video/image
#[derive(Debug, Clone)]
pub enum CalibrationMode {
    /// Real-time calibration with live camera
    RealTime {
        /// Camera resolution
        camera_resolution: (u32, u32),
    },
    /// Process from image file (photo)
    Photo {
        /// Path to image file
        image_path: std::path::PathBuf,
    },
    /// Decode from recorded video file
    Video {
        /// Path to video file
        video_path: std::path::PathBuf,
    },
}

/// Current phase of calibration
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationPhase {
    /// Idle, not calibrating
    Idle,
    /// Countdown before showing patterns (seconds remaining)
    Countdown { seconds_remaining: u32 },
    /// Showing all patterns simultaneously (ready for capture)
    ShowingAllPatterns,
    /// Captured frame being processed
    Processing {
        /// Current step
        current: usize,
        /// Total steps
        total: usize,
    },
    /// Building the quad map from detections
    BuildingMap,
    /// Calibration complete
    Complete,
    /// Error occurred
    Error(CalibrationError),
}

/// Calibration error types
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationError {
    /// Camera error
    CameraError(String),
    /// Image/video decode error
    DecodeError(String),
    /// Marker detection error
    DetectionError(String),
    /// No markers detected
    NoMarkersDetected,
    /// Missing displays
    MissingDisplays { expected: usize, found: usize },
    /// Wrong marker detected
    WrongMarker { expected: u32, found: u32 },
    /// Timeout waiting for frame
    Timeout,
    /// User cancelled
    Cancelled,
    /// IO error
    IoError(String),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CameraError(e) => write!(f, "Camera error: {}", e),
            Self::DecodeError(e) => write!(f, "Decode error: {}", e),
            Self::DetectionError(e) => write!(f, "Detection error: {}", e),
            Self::NoMarkersDetected => write!(f, "No ArUco markers detected in frame"),
            Self::MissingDisplays { expected, found } => {
                write!(f, "Missing displays: expected {}, found {}", expected, found)
            }
            Self::WrongMarker { expected, found } => {
                write!(f, "Wrong marker: expected #{}, found #{}", expected, found)
            }
            Self::Timeout => write!(f, "Timeout waiting for frame"),
            Self::Cancelled => write!(f, "Calibration cancelled by user"),
            Self::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// Status returned from update()
#[derive(Debug)]
pub enum CalibrationStatus {
    /// Calibration in progress, continue calling update()
    InProgress,
    /// Patterns are showing, ready for user to capture
    ReadyForCapture,
    /// Captured frame, processing markers
    Processing,
    /// Calibration complete with config
    Complete(VideoWallConfig),
    /// Calibration failed
    Error(CalibrationError),
}

/// Captured frame with metadata
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Frame data (RGBA)
    pub data: Vec<u8>,
    /// Frame dimensions
    pub width: u32,
    pub height: u32,
    /// When captured
    pub timestamp: Instant,
}

/// Detected marker with display association
#[derive(Debug, Clone)]
pub struct DisplayDetection {
    /// Display ID this detection is for
    pub display_id: u32,
    /// Detected marker
    pub marker: Option<DetectedMarker>,
    /// Frame dimensions when detected
    pub frame_width: u32,
    pub frame_height: u32,
}

/// Configuration for calibration timing
#[derive(Debug, Clone, Copy)]
pub struct CalibrationTiming {
    /// Countdown duration in seconds
    pub countdown_seconds: u32,
    /// Timeout for frame capture (milliseconds)
    pub capture_timeout_ms: u64,
}

impl Default for CalibrationTiming {
    fn default() -> Self {
        Self {
            countdown_seconds: 3,
            capture_timeout_ms: 30000, // 30 seconds
        }
    }
}

/// Configuration for marker display
#[derive(Debug, Clone, Copy)]
pub struct MarkerDisplayConfig {
    /// Marker size as percentage of display size (0-1)
    pub marker_size_percent: f32,
    /// Margin around marker as percentage of display size
    pub margin_percent: f32,
}

impl Default for MarkerDisplayConfig {
    fn default() -> Self {
        Self {
            marker_size_percent: 0.75, // 75% of display
            margin_percent: 0.125,     // 12.5% margin on each side
        }
    }
}

/// Calibration controller state machine
#[derive(Debug)]
pub struct CalibrationController {
    /// Current calibration mode
    mode: CalibrationMode,
    /// Current phase
    phase: CalibrationPhase,
    /// Grid size being calibrated (max possible displays)
    grid_size: GridSize,
    /// Whether to use auto-detect mode (detect any number of displays)
    auto_detect: bool,
    /// Timing configuration
    timing: CalibrationTiming,
    /// Marker display configuration
    marker_config: MarkerDisplayConfig,
    /// ArUco generator
    generator: ArUcoGenerator,
    /// Generated pattern showing all markers
    all_patterns_frame: Option<image::RgbaImage>,
    /// Captured frame
    captured_frame: Option<CapturedFrame>,
    /// Detected markers from processing
    detections: Vec<DisplayDetection>,
    /// When current phase started
    phase_start: Instant,
    /// Calibration start time (for duration tracking)
    calibration_start: Option<Instant>,
}

impl CalibrationController {
    /// Create a new calibration controller
    pub fn new() -> Self {
        Self {
            mode: CalibrationMode::RealTime {
                camera_resolution: (1920, 1080),
            },
            phase: CalibrationPhase::Idle,
            grid_size: GridSize::default(),
            auto_detect: false,
            timing: CalibrationTiming::default(),
            marker_config: MarkerDisplayConfig::default(),
            generator: ArUcoGenerator::new(ArUcoDictionary::default()),
            all_patterns_frame: None,
            captured_frame: None,
            detections: Vec::new(),
            phase_start: Instant::now(),
            calibration_start: None,
        }
    }

    /// Enable auto-detect mode (detect any number of displays)
    pub fn with_auto_detect(mut self, enabled: bool) -> Self {
        self.auto_detect = enabled;
        self
    }

    /// Configure timing
    pub fn with_timing(mut self, timing: CalibrationTiming) -> Self {
        self.timing = timing;
        self
    }

    /// Configure marker display
    pub fn with_marker_config(mut self, config: MarkerDisplayConfig) -> Self {
        self.marker_config = config;
        self
    }

    /// Start real-time calibration
    pub fn start_realtime(
        &mut self,
        grid_size: GridSize,
        camera_resolution: (u32, u32),
        output_resolution: (u32, u32),
    ) -> anyhow::Result<()> {
        let total_displays = grid_size.total_displays();
        
        // Select appropriate dictionary
        let dictionary = ArUcoDictionary::for_grid_size(grid_size.columns, grid_size.rows);
        self.generator = ArUcoGenerator::new(dictionary);
        
        // Generate single frame with all markers
        self.all_patterns_frame = Some(self.generator.generate_all_markers_frame(
            (grid_size.columns, grid_size.rows),
            output_resolution,
            self.marker_config.marker_size_percent,
        )?);
        
        self.grid_size = grid_size;
        self.mode = CalibrationMode::RealTime { camera_resolution };
        self.phase = CalibrationPhase::Countdown {
            seconds_remaining: self.timing.countdown_seconds,
        };
        self.captured_frame = None;
        self.detections.clear();
        self.calibration_start = Some(Instant::now());
        
        log::info!(
            "Starting real-time calibration: {} displays, {:?} dictionary, {}% marker size",
            total_displays,
            dictionary,
            (self.marker_config.marker_size_percent * 100.0) as u32
        );
        
        Ok(())
    }

    /// Start photo calibration from image file
    pub fn start_from_photo(
        &mut self,
        grid_size: GridSize,
        image_path: &Path,
        output_resolution: (u32, u32),
    ) -> anyhow::Result<()> {
        if !image_path.exists() {
            anyhow::bail!("Image file not found: {:?}", image_path);
        }
        
        let total_displays = grid_size.total_displays();
        
        // Select appropriate dictionary
        let dictionary = ArUcoDictionary::for_grid_size(grid_size.columns, grid_size.rows);
        self.generator = ArUcoGenerator::new(dictionary);
        
        // Generate reference patterns (for display during setup)
        self.all_patterns_frame = Some(self.generator.generate_all_markers_frame(
            (grid_size.columns, grid_size.rows),
            output_resolution,
            self.marker_config.marker_size_percent,
        )?);
        
        self.grid_size = grid_size;
        self.mode = CalibrationMode::Photo {
            image_path: image_path.to_path_buf(),
        };
        self.phase = CalibrationPhase::Processing { current: 0, total: 1 };
        self.captured_frame = None;
        self.detections.clear();
        self.calibration_start = Some(Instant::now());
        
        log::info!(
            "Starting photo calibration: {} displays from {:?}",
            total_displays,
            image_path
        );
        
        Ok(())
    }

    /// Get current phase
    pub fn phase(&self) -> CalibrationPhase {
        self.phase.clone()
    }

    /// Check if calibration is in progress
    pub fn is_active(&self) -> bool {
        !matches!(self.phase, CalibrationPhase::Idle | CalibrationPhase::Complete | CalibrationPhase::Error(_))
    }

    /// Check if patterns are showing and ready for capture
    pub fn is_ready_for_capture(&self) -> bool {
        matches!(self.phase, CalibrationPhase::ShowingAllPatterns)
    }

    /// Get the current pattern frame (all markers displayed)
    pub fn current_pattern(&self) -> Option<&image::RgbaImage> {
        self.all_patterns_frame.as_ref()
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        match self.phase {
            CalibrationPhase::Idle => 0.0,
            CalibrationPhase::Countdown { seconds_remaining } => {
                let total = self.timing.countdown_seconds as f32;
                let remaining = seconds_remaining as f32;
                (total - remaining) / total * 0.2 // First 20%
            }
            CalibrationPhase::ShowingAllPatterns => 0.2,
            CalibrationPhase::Processing { current, total } => {
                0.4 + (current as f32 / total as f32) * 0.4 // 40-80%
            }
            CalibrationPhase::BuildingMap => 0.9,
            CalibrationPhase::Complete => 1.0,
            CalibrationPhase::Error(_) => 0.0,
        }
    }

    /// Get grid size
    pub fn grid_size(&self) -> GridSize {
        self.grid_size
    }

    /// Get marker display configuration
    pub fn marker_config(&self) -> &MarkerDisplayConfig {
        &self.marker_config
    }

    /// Update marker display configuration (only valid when idle)
    pub fn set_marker_config(&mut self, config: MarkerDisplayConfig) {
        if matches!(self.phase, CalibrationPhase::Idle) {
            self.marker_config = config;
        }
    }

    /// Cancel calibration
    pub fn cancel(&mut self) {
        self.phase = CalibrationPhase::Error(CalibrationError::Cancelled);
        log::info!("Calibration cancelled by user");
    }

    /// Trigger capture (user clicked capture button)
    pub fn trigger_capture(&mut self) {
        if matches!(self.phase, CalibrationPhase::ShowingAllPatterns) {
            self.phase = CalibrationPhase::Processing { current: 0, total: 2 };
            log::info!("Capture triggered, waiting for frame...");
        }
    }

    /// Submit a captured frame (called from camera callback)
    pub fn submit_frame(&mut self, frame_data: Vec<u8>, width: u32, height: u32) {
        if !matches!(self.phase, CalibrationPhase::Processing { .. }) {
            return;
        }

        self.captured_frame = Some(CapturedFrame {
            data: frame_data,
            width,
            height,
            timestamp: Instant::now(),
        });
        
        // Move to processing step 1
        self.phase = CalibrationPhase::Processing { current: 1, total: 2 };
        
        log::info!("Frame captured: {}x{}", width, height);
    }

    /// Update calibration state (call regularly from main loop)
    pub fn update(&mut self) -> CalibrationStatus {
        match &mut self.phase {
            CalibrationPhase::Idle => CalibrationStatus::InProgress,
            
            CalibrationPhase::Countdown { seconds_remaining } => {
                let elapsed = self.phase_start.elapsed().as_secs() as u32;
                let remaining = self.timing.countdown_seconds.saturating_sub(elapsed);
                
                if remaining == 0 {
                    // Start showing all patterns
                    self.phase = CalibrationPhase::ShowingAllPatterns;
                    self.phase_start = Instant::now();
                    log::info!("Showing all patterns - ready for capture");
                } else if remaining != *seconds_remaining {
                    self.phase = CalibrationPhase::Countdown { seconds_remaining: remaining };
                    log::debug!("Countdown: {} seconds remaining", remaining);
                }
                
                CalibrationStatus::InProgress
            }
            
            CalibrationPhase::ShowingAllPatterns => {
                // Check for timeout
                if self.phase_start.elapsed().as_millis() as u64 > self.timing.capture_timeout_ms {
                    self.phase = CalibrationPhase::Error(CalibrationError::Timeout);
                    return CalibrationStatus::Error(CalibrationError::Timeout);
                }
                
                CalibrationStatus::ReadyForCapture
            }
            
            CalibrationPhase::Processing { current, total } => {
                // Process the captured frame
                if *current == 0 {
                    // Waiting for frame submission
                    if self.captured_frame.is_some() {
                        *current = 1;
                    }
                    CalibrationStatus::Processing
                } else {
                    // Process markers
                    match self.process_captured_frame() {
                        Ok(_) => {
                            self.phase = CalibrationPhase::BuildingMap;
                            CalibrationStatus::Processing
                        }
                        Err(e) => {
                            self.phase = CalibrationPhase::Error(e.clone());
                            CalibrationStatus::Error(e)
                        }
                    }
                }
            }
            
            CalibrationPhase::BuildingMap => {
                // Build the quad map from detections
                match self.build_quad_map() {
                    Ok(config) => {
                        self.phase = CalibrationPhase::Complete;
                        CalibrationStatus::Complete(config)
                    }
                    Err(e) => {
                        self.phase = CalibrationPhase::Error(e);
                        CalibrationStatus::Error(self.phase.clone().into_error().unwrap())
                    }
                }
            }
            
            CalibrationPhase::Complete => CalibrationStatus::InProgress,
            
            CalibrationPhase::Error(ref e) => {
                CalibrationStatus::Error(e.clone())
            }
        }
    }

    /// Process the captured frame to detect markers
    fn process_captured_frame(&mut self) -> Result<(), CalibrationError> {
        let frame = self.captured_frame.as_ref()
            .ok_or(CalibrationError::CameraError("No frame captured".to_string()))?;
        
        // Convert frame data to image
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
            .ok_or(CalibrationError::DecodeError("Invalid frame data".to_string()))?;
        
        // Detect all markers in the frame
        let detector = super::ArUcoDetector::new(self.generator.dictionary());
        
        #[cfg(feature = "opencv")]
        {
            // Convert to OpenCV Mat for detection
            let mat = Self::image_to_mat(&image)
                .map_err(|e| CalibrationError::DetectionError(e.to_string()))?;
            
            let markers = detector.detect_markers(&mat)
                .map_err(|e| CalibrationError::DetectionError(e.to_string()))?;
            
            if markers.is_empty() {
                return Err(CalibrationError::NoMarkersDetected);
            }
            
            // Create detections for all found markers
            for marker in markers {
                self.detections.push(DisplayDetection {
                    display_id: marker.id,
                    marker: Some(marker),
                    frame_width: frame.width,
                    frame_height: frame.height,
                });
            }
        }
        
        #[cfg(not(feature = "opencv"))]
        {
            // Fallback: can't detect without OpenCV
            return Err(CalibrationError::DetectionError(
                "OpenCV feature required for marker detection".to_string()
            ));
        }
        
        log::info!("Detected {} markers in frame", self.detections.len());
        Ok(())
    }

    /// Convert image::RgbaImage to OpenCV Mat
    #[cfg(feature = "opencv")]
    fn image_to_mat(image: &image::RgbaImage) -> anyhow::Result<opencv::core::Mat> {
        use opencv::core::{Mat, CV_8UC4};
        
        let width = image.width() as i32;
        let height = image.height() as i32;
        let data = image.as_raw();
        
        let mat = Mat::from_slice(data)?;
        let mat = mat.reshape(4, height)?;
        
        Ok(mat)
    }

    /// Build quad map from detections
    fn build_quad_map(&self) -> Result<VideoWallConfig, CalibrationError> {
        // Get camera resolution from captured frame
        let camera_resolution = self.captured_frame.as_ref()
            .map(|f| (f.width, f.height))
            .unwrap_or((1920, 1080));
        
        // Determine effective grid size
        // In auto-detect mode, we use the original grid size but only enable detected displays
        // The quad mapper will create quads for detected markers only
        let effective_grid_size = if self.auto_detect {
            // Auto-detect: use whatever grid size was configured, missing displays are disabled
            self.grid_size
        } else {
            self.grid_size
        };
        
        // Use QuadMapper to build quads
        let config = QuadMapConfig::default();
        let result = QuadMapper::build_quads(
            &self.detections,
            effective_grid_size,
            camera_resolution,
            Some(config),
        );
        
        // Log warnings
        for warning in &result.warnings {
            log::warn!("Quad mapping: {}", warning);
        }
        
        // Check if we have any quads
        if result.quads.is_empty() {
            return Err(CalibrationError::DetectionError(
                "No valid quads could be built from detections".to_string()
            ));
        }
        
        // Log detection results
        let detected_count = result.quads.len();
        let expected_count = effective_grid_size.total_displays() as usize;
        
        if detected_count < expected_count {
            log::info!(
                "Auto-detect mode: Found {} of {} possible displays",
                detected_count,
                expected_count
            );
            if !result.missing_displays.is_empty() {
                log::info!(
                    "Displays not detected (will be disabled): {:?}",
                    result.missing_displays
                );
            }
        } else {
            log::info!(
                "Auto-detect mode: Found all {} displays",
                detected_count
            );
        }
        
        // Calculate calibration duration
        let duration = self.calibration_start
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        
        // Get camera info from mode
        let (camera_source, _camera_resolution) = match &self.mode {
            CalibrationMode::RealTime { camera_resolution } => {
                ("Real-time Camera".to_string(), *camera_resolution)
            }
            CalibrationMode::Photo { image_path } => {
                (image_path.to_string_lossy().to_string(), (0, 0))
            }
            CalibrationMode::Video { video_path } => {
                (video_path.to_string_lossy().to_string(), (0, 0))
            }
        };
        
        // Calculate average confidence from detections
        let avg_confidence = if !self.detections.is_empty() {
            let total_confidence: f32 = self.detections
                .iter()
                .filter_map(|d| d.marker.as_ref().map(|m| m.confidence))
                .sum();
            let valid_count = self.detections.iter().filter(|d| d.marker.is_some()).count() as f32;
            if valid_count > 0.0 {
                total_confidence / valid_count
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        let info = CalibrationInfo {
            date: chrono::Utc::now().to_rfc3339(),
            camera_source,
            camera_resolution,
            marker_dictionary: format!("{:?}", self.generator.dictionary()),
            avg_detection_confidence: avg_confidence,
            calibration_duration_secs: duration,
        };
        
        Ok(VideoWallConfig::from_quads(
            result.quads,
            effective_grid_size,
            camera_resolution,
            info,
        ))
    }

    /// Get detected markers (for preview/debug)
    pub fn detections(&self) -> &[DisplayDetection] {
        &self.detections
    }
}

impl Default for CalibrationController {
    fn default() -> Self {
        Self::new()
    }
}

impl CalibrationPhase {
    /// Convert to error if this is an error phase
    fn into_error(self) -> Option<CalibrationError> {
        match self {
            Self::Error(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_timing_default() {
        let timing = CalibrationTiming::default();
        assert_eq!(timing.countdown_seconds, 3);
        assert_eq!(timing.capture_timeout_ms, 30000);
    }

    #[test]
    fn test_marker_config_default() {
        let config = MarkerDisplayConfig::default();
        assert_eq!(config.marker_size_percent, 0.75);
        assert_eq!(config.margin_percent, 0.125);
    }

    #[test]
    fn test_controller_new() {
        let controller = CalibrationController::new();
        assert!(matches!(controller.phase(), CalibrationPhase::Idle));
        assert!(!controller.is_active());
        assert!(!controller.is_ready_for_capture());
    }

    #[test]
    fn test_start_realtime() {
        let mut controller = CalibrationController::new();
        let result = controller.start_realtime(
            GridSize::new(3, 3),
            (1920, 1080),
            (1920, 1080),
        );
        
        assert!(result.is_ok());
        assert!(matches!(controller.phase(), CalibrationPhase::Countdown { .. }));
        assert!(controller.is_active());
        assert_eq!(controller.grid_size().total_displays(), 9);
        assert!(controller.current_pattern().is_some());
    }

    #[test]
    fn test_progress() {
        let mut controller = CalibrationController::new();
        
        // Idle
        assert_eq!(controller.progress(), 0.0);
        
        // Start calibration
        controller.start_realtime(
            GridSize::new(3, 3),
            (1920, 1080),
            (1920, 1080),
        ).unwrap();
        
        // Countdown
        assert!(controller.progress() >= 0.0 && controller.progress() <= 0.2);
        
        // Move to showing patterns
        controller.phase = CalibrationPhase::ShowingAllPatterns;
        assert_eq!(controller.progress(), 0.2);
        
        // Complete
        controller.phase = CalibrationPhase::Complete;
        assert_eq!(controller.progress(), 1.0);
    }

    #[test]
    fn test_is_ready_for_capture() {
        let mut controller = CalibrationController::new();
        
        // Not ready in idle
        assert!(!controller.is_ready_for_capture());
        
        // Start calibration
        controller.start_realtime(
            GridSize::new(3, 3),
            (1920, 1080),
            (1920, 1080),
        ).unwrap();
        
        // Not ready in countdown
        assert!(!controller.is_ready_for_capture());
        
        // Ready when showing patterns
        controller.phase = CalibrationPhase::ShowingAllPatterns;
        assert!(controller.is_ready_for_capture());
    }

    #[test]
    fn test_trigger_capture() {
        let mut controller = CalibrationController::new();
        controller.start_realtime(
            GridSize::new(3, 3),
            (1920, 1080),
            (1920, 1080),
        ).unwrap();
        
        // Move to showing patterns
        controller.phase = CalibrationPhase::ShowingAllPatterns;
        
        // Trigger capture
        controller.trigger_capture();
        
        assert!(matches!(controller.phase(), CalibrationPhase::Processing { .. }));
    }

    #[test]
    fn test_cancel() {
        let mut controller = CalibrationController::new();
        controller.start_realtime(
            GridSize::new(3, 3),
            (1920, 1080),
            (1920, 1080),
        ).unwrap();
        
        controller.cancel();
        
        assert!(matches!(controller.phase(), CalibrationPhase::Error(_)));
        assert!(!controller.is_active());
    }

    #[test]
    fn test_calibration_error_display() {
        let error = CalibrationError::MissingDisplays { expected: 4, found: 2 };
        let msg = format!("{}", error);
        assert!(msg.contains("Missing displays"));
        assert!(msg.contains("expected 4"));
        assert!(msg.contains("found 2"));

        let error2 = CalibrationError::NoMarkersDetected;
        let msg2 = format!("{}", error2);
        assert!(msg2.contains("No ArUco markers"));
    }
}
