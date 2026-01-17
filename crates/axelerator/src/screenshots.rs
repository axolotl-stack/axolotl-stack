//! Screenshot/showcase image management for Xbox Live gallery.
//!
//! Handles uploading showcase images to the Xbox Live Gallery API,
//! with support for cycling through multiple images from a directory.

use crate::config::ScreenshotConfig;
use anyhow::{Context, Result};
use axolotl_xbl::GalleryClient;
use rand::seq::SliceRandom;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Supported image file extensions.
const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

/// Manages screenshot uploads to Xbox Live Gallery.
pub struct ScreenshotManager {
    config: ScreenshotConfig,
    gallery_client: GalleryClient,
    /// List of image paths (either single or from directory).
    images: Vec<PathBuf>,
    /// Current index in the image cycle.
    current_index: usize,
    /// Last upload time.
    last_upload: Option<Instant>,
    /// Whether initial upload has been done.
    initial_upload_done: bool,
}

impl ScreenshotManager {
    /// Create a new screenshot manager.
    pub fn new(config: ScreenshotConfig) -> Self {
        Self {
            config,
            gallery_client: GalleryClient::new(),
            images: Vec::new(),
            current_index: 0,
            last_upload: None,
            initial_upload_done: false,
        }
    }

    /// Initialize the manager by discovering available images.
    pub fn init(&mut self) -> Result<()> {
        if !self.config.enabled {
            debug!("Screenshot feature disabled");
            return Ok(());
        }

        // Priority: directory > single path
        // Clone the path values to avoid borrow checker issues
        let directory = self.config.directory.clone();
        let path = self.config.path.clone();

        if let Some(dir) = directory {
            self.load_images_from_directory(&dir)?;
        } else if let Some(p) = path {
            self.load_single_image(&p)?;
        } else {
            warn!("Screenshots enabled but no path or directory configured");
            return Ok(());
        }

        if self.images.is_empty() {
            warn!("No valid images found for screenshots");
        } else {
            info!(count = self.images.len(), "Loaded screenshot images");

            // Optionally randomize order
            if self.config.randomize_order && self.images.len() > 1 {
                let mut rng = rand::thread_rng();
                self.images.shuffle(&mut rng);
                debug!("Randomized image order");
            }
        }

        Ok(())
    }

    /// Load a single image path.
    fn load_single_image(&mut self, path: &str) -> Result<()> {
        let path = PathBuf::from(path);
        if path.exists() {
            info!(path = %path.display(), "Found screenshot image");
            self.images.push(path);
        } else {
            warn!(path = %path.display(), "Screenshot file not found");
        }
        Ok(())
    }

    /// Load images from a directory.
    fn load_images_from_directory(&mut self, dir: &str) -> Result<()> {
        let dir_path = Path::new(dir);
        if !dir_path.exists() {
            warn!(path = %dir, "Screenshot directory not found");
            return Ok(());
        }

        if !dir_path.is_dir() {
            warn!(path = %dir, "Screenshot path is not a directory");
            return Ok(());
        }

        let entries = std::fs::read_dir(dir_path)
            .with_context(|| format!("Failed to read screenshot directory: {}", dir))?;

        let mut image_paths: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension()
            {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str()) {
                    debug!(path = %path.display(), "Found image");
                    image_paths.push(path);
                }
            }
        }

        // Sort alphabetically for deterministic order (unless randomize is enabled)
        image_paths.sort();

        info!(
            directory = %dir,
            count = image_paths.len(),
            "Loaded images from directory"
        );

        self.images = image_paths;
        Ok(())
    }

    /// Check if an upload is needed based on timing.
    pub fn should_upload(&self) -> bool {
        if !self.config.enabled || self.images.is_empty() {
            return false;
        }

        // Initial upload on startup
        if self.config.upload_on_startup && !self.initial_upload_done {
            return true;
        }

        // Cycling (only if we have multiple images)
        if self.images.len() > 1
            && let Some(last) = self.last_upload
        {
            return last.elapsed().as_secs() >= self.config.cycle_interval;
        }

        false
    }

    /// Get the current image path to upload.
    pub fn current_image(&self) -> Option<&PathBuf> {
        self.images.get(self.current_index)
    }

    /// Advance to the next image in the cycle.
    pub fn advance(&mut self) {
        if self.images.len() > 1 {
            self.current_index = (self.current_index + 1) % self.images.len();
            debug!(
                index = self.current_index,
                total = self.images.len(),
                "Advanced to next image"
            );
        }
    }

    /// Mark that an upload was performed.
    pub fn mark_uploaded(&mut self) {
        self.last_upload = Some(Instant::now());
        self.initial_upload_done = true;
    }

    /// Upload the current screenshot to Xbox Live Gallery.
    ///
    /// Returns `Ok(true)` if uploaded, `Ok(false)` if skipped (already exists), `Err` on failure.
    pub async fn upload_screenshot(&mut self, mc_token: &str, xuid: &str) -> Result<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        let image_path = match self.current_image() {
            Some(path) => path.clone(),
            None => {
                debug!("No image to upload");
                return Ok(false);
            }
        };

        info!(path = %image_path.display(), "Uploading showcase screenshot");

        match self
            .gallery_client
            .set_showcase(mc_token, xuid, &image_path)
            .await
        {
            Ok(result) => {
                self.mark_uploaded();

                if result.uploaded {
                    info!(
                        image_id = %result.image_id,
                        deleted = result.deleted_count,
                        "Screenshot uploaded successfully"
                    );
                } else {
                    debug!(
                        image_id = %result.image_id,
                        "Screenshot already set (hash match)"
                    );
                }

                // Advance for next cycle
                self.advance();

                Ok(result.uploaded)
            }
            Err(e) => {
                // Check for rate limiting
                if matches!(e, axolotl_xbl::XblError::RateLimited) {
                    warn!("Screenshot upload rate limited, will retry later");
                    // Don't mark as uploaded so we retry
                    return Ok(false);
                }

                error!("Failed to upload screenshot: {}", e);
                Err(e.into())
            }
        }
    }

    /// Get the configured cycle interval in seconds.
    pub fn cycle_interval_secs(&self) -> u64 {
        self.config.cycle_interval
    }

    /// Check if screenshot feature is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && !self.images.is_empty()
    }

    /// Get the number of loaded images.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_disabled() {
        let config = ScreenshotConfig::default();
        let manager = ScreenshotManager::new(config);
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_cycle_interval() {
        let config = ScreenshotConfig {
            enabled: true,
            cycle_interval: 120,
            ..Default::default()
        };
        let manager = ScreenshotManager::new(config);
        assert_eq!(manager.cycle_interval_secs(), 120);
    }
}
