//! Xbox Live Gallery API client for showcase images.
//!
//! This module provides functionality to upload, retrieve, and manage
//! showcase/screenshot images on Xbox Live's Minecraft Gallery service.
//!
//! # Example
//!
//! ```no_run
//! use axolotl_xbl::GalleryClient;
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = GalleryClient::new();
//!
//! // Upload a screenshot
//! let mc_token = "your_mc_token";
//! let xuid = "your_xuid";
//!
//! // Set a showcase image (will replace existing ones)
//! let result = client.set_showcase(
//!     mc_token,
//!     xuid,
//!     Path::new("screenshot.jpg"),
//! ).await?;
//!
//! if result.uploaded {
//!     println!("Uploaded new image: {}", result.image_id);
//! } else {
//!     println!("Image already exists, skipping upload");
//! }
//! # Ok(())
//! # }
//! ```

use crate::constants::endpoints;
use crate::error::{XblError, XblResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::{debug, info, warn};

/// Response from gallery image upload.
#[derive(Debug, Deserialize)]
pub struct GalleryUploadResponse {
    pub result: GalleryImage,
}

/// Gallery image metadata.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryImage {
    /// Unique image identifier.
    pub id: String,
    /// Whether this is the featured/showcase image.
    pub is_featured: bool,
    /// When the image was last modified on the server.
    pub last_modified: String,
    /// When the image was originally taken/created.
    pub taken_time: String,
    /// URL to download the image.
    pub url: String,
}

/// Response from getting gallery images.
#[derive(Debug, Deserialize)]
pub struct GalleryResponse {
    pub result: GalleryResult,
}

/// Gallery result containing showcased images.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryResult {
    pub showcased_images: Vec<GalleryImage>,
}

/// Result of a set_showcase operation.
#[derive(Debug)]
pub struct SetShowcaseResult {
    /// The image ID (either newly uploaded or existing).
    pub image_id: String,
    /// Whether a new image was uploaded (false if image already existed).
    pub uploaded: bool,
    /// Number of old images that were deleted.
    pub deleted_count: usize,
}

/// Xbox Live Gallery client for managing showcase images.
pub struct GalleryClient {
    client: reqwest::Client,
}

impl GalleryClient {
    /// Create a new Gallery client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Upload an image to the gallery.
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token (from PlayFab session start).
    /// * `image_data` - Raw image bytes (JPEG recommended).
    /// * `is_featured` - Whether this is a featured/showcase image.
    /// * `taken_time` - When the image was taken/created.
    ///
    /// # Returns
    /// The uploaded image metadata on success.
    pub async fn upload_image(
        &self,
        mc_token: &str,
        image_data: &[u8],
        is_featured: bool,
        taken_time: DateTime<Utc>,
    ) -> XblResult<GalleryImage> {
        debug!(
            size = image_data.len(),
            is_featured, "Uploading gallery image"
        );

        let response = self
            .client
            .post(endpoints::GALLERY)
            .header("Authorization", mc_token)
            .header("Content-Type", "application/octet-stream")
            .header("x-ms-showcased-featured", is_featured.to_string())
            .header("x-ms-showcased-timetaken", taken_time.to_rfc3339())
            .body(image_data.to_vec())
            .send()
            .await?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(XblError::RateLimited);
        }

        // API returns 202 Accepted on success
        if status.as_u16() != 202 {
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Failed to upload gallery image ({}): {}",
                status, body
            )));
        }

        let upload_response: GalleryUploadResponse = response.json().await?;
        info!(id = %upload_response.result.id, "Gallery image uploaded");

        Ok(upload_response.result)
    }

    /// Get all showcase images for a user.
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token.
    /// * `xuid` - The Xbox User ID to get images for.
    ///
    /// # Returns
    /// List of gallery images for the user.
    pub async fn get_images(&self, mc_token: &str, xuid: &str) -> XblResult<Vec<GalleryImage>> {
        let url = format!("{}/xuid/{}", endpoints::GALLERY, xuid);

        debug!(xuid, "Getting gallery images");

        let response = self
            .client
            .get(&url)
            .header("Authorization", mc_token)
            .send()
            .await?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(XblError::RateLimited);
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Failed to get gallery images ({}): {}",
                status, body
            )));
        }

        let gallery_response: GalleryResponse = response.json().await?;
        debug!(
            count = gallery_response.result.showcased_images.len(),
            "Retrieved gallery images"
        );

        Ok(gallery_response.result.showcased_images)
    }

    /// Delete an image from the gallery.
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token.
    /// * `image_id` - The ID of the image to delete.
    pub async fn delete_image(&self, mc_token: &str, image_id: &str) -> XblResult<()> {
        let url = format!("{}/{}", endpoints::GALLERY, image_id);

        debug!(image_id, "Deleting gallery image");

        let response = self
            .client
            .delete(&url)
            .header("Authorization", mc_token)
            .send()
            .await?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(XblError::RateLimited);
        }

        if !status.is_success() && status.as_u16() != 404 {
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Failed to delete gallery image ({}): {}",
                status, body
            )));
        }

        info!(image_id, "Gallery image deleted");
        Ok(())
    }

    /// Download an image's raw bytes.
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token.
    /// * `image` - The gallery image to download.
    ///
    /// # Returns
    /// The raw image bytes.
    pub async fn download_image(&self, mc_token: &str, image: &GalleryImage) -> XblResult<Vec<u8>> {
        debug!(id = %image.id, url = %image.url, "Downloading gallery image");

        let response = self
            .client
            .get(&image.url)
            .header("Authorization", mc_token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to download gallery image: {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Set a showcase image, replacing any existing ones.
    ///
    /// This method:
    /// 1. Gets existing gallery images
    /// 2. Compares hashes to avoid re-uploading the same image
    /// 3. Uploads the new image if it's different
    /// 4. Deletes all other images to keep only the new one
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token.
    /// * `xuid` - The Xbox User ID.
    /// * `image_path` - Path to the image file (JPEG recommended, 1200x675).
    ///
    /// # Returns
    /// Result containing the image ID and whether it was newly uploaded.
    pub async fn set_showcase(
        &self,
        mc_token: &str,
        xuid: &str,
        image_path: &Path,
    ) -> XblResult<SetShowcaseResult> {
        // Read the new image file (using blocking read in spawn_blocking for safety)
        let path = image_path.to_path_buf();
        let new_image_data = tokio::task::spawn_blocking(move || std::fs::read(&path))
            .await
            .map_err(|e| XblError::Other(format!("Task join error: {}", e)))?
            .map_err(|e| {
                XblError::Other(format!("Failed to read image file {:?}: {}", image_path, e))
            })?;

        self.set_showcase_from_bytes(mc_token, xuid, &new_image_data)
            .await
    }

    /// Set a showcase image from raw bytes, replacing any existing ones.
    ///
    /// # Arguments
    /// * `mc_token` - The Minecraft authorization token.
    /// * `xuid` - The Xbox User ID.
    /// * `image_data` - Raw image bytes (JPEG recommended).
    ///
    /// # Returns
    /// Result containing the image ID and whether it was newly uploaded.
    pub async fn set_showcase_from_bytes(
        &self,
        mc_token: &str,
        xuid: &str,
        image_data: &[u8],
    ) -> XblResult<SetShowcaseResult> {
        // Get existing images
        let existing_images = self.get_images(mc_token, xuid).await?;

        // Hash the new image data
        let new_hash = Self::hash_image_data(image_data);
        debug!(hash = %new_hash, "New image hash");

        // Check if any existing image has the same hash
        let mut matching_image_id: Option<String> = None;

        for existing in &existing_images {
            match self.download_image(mc_token, existing).await {
                Ok(existing_data) => {
                    let existing_hash = Self::hash_image_data(&existing_data);
                    if existing_hash == new_hash {
                        info!(id = %existing.id, "Showcase image already set, skipping upload");
                        matching_image_id = Some(existing.id.clone());
                        break;
                    }
                }
                Err(e) => {
                    warn!(id = %existing.id, error = %e, "Failed to download existing image for comparison");
                }
            }
        }

        let (image_id, uploaded) = if let Some(id) = matching_image_id {
            (id, false)
        } else {
            // Upload new image
            let uploaded_image = self
                .upload_image(mc_token, image_data, true, Utc::now())
                .await?;
            (uploaded_image.id, true)
        };

        // Delete all other images
        let mut deleted_count = 0;
        for existing in existing_images {
            if existing.id != image_id {
                if let Err(e) = self.delete_image(mc_token, &existing.id).await {
                    warn!(id = %existing.id, error = %e, "Failed to delete old gallery image");
                } else {
                    deleted_count += 1;
                }
            }
        }

        Ok(SetShowcaseResult {
            image_id,
            uploaded,
            deleted_count,
        })
    }

    /// Hash image data using SHA-256.
    /// We use SHA-256 instead of SHA-1 (like the Java version) for better security.
    fn hash_image_data(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        hex::encode(result)
    }
}

impl Default for GalleryClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_image_data() {
        let data = b"test image data";
        let hash = GalleryClient::hash_image_data(data);
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex characters
    }

    #[test]
    fn test_hash_consistency() {
        let data = b"same data";
        let hash1 = GalleryClient::hash_image_data(data);
        let hash2 = GalleryClient::hash_image_data(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_data() {
        let hash1 = GalleryClient::hash_image_data(b"data1");
        let hash2 = GalleryClient::hash_image_data(b"data2");
        assert_ne!(hash1, hash2);
    }
}
