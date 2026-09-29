//! Domain models, value types, and traits for station-icon artifacts.
//!
//! This module defines normalized WebP icon representations, content-addressed keys,
//! validation boundaries, and abstract storage and fetcher traits. Image normalization
//! and raster-to-WebP conversion live here without any dependencies on SQL or external
//! HTTP transport.

use std::{error::Error, fmt, io::Cursor};

use async_trait::async_trait;
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage, imageops::FilterType};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Maximum accepted encoded source or administrator-upload byte size.
pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_SOURCE_PIXELS: u32 = 1_048_576;
pub(crate) const MAX_SOURCE_DIMENSION: u32 = 1_024;
pub(crate) const ICON_DIMENSION: u32 = 256;

/// A validated content-addressed key for one normalized WebP artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconStorageKey(String);

impl IconStorageKey {
    /// Builds the canonical filename for a SHA-256 content hash.
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        Self(format!("{}.webp", hex(hash)))
    }

    /// Parses a canonical storage key without allowing paths or alternate extensions.
    pub fn parse(value: &str) -> Result<Self, IconStorageError> {
        let valid = value.len() == 69
            && value.ends_with(".webp")
            && value[..64].bytes().all(|byte| byte.is_ascii_hexdigit());
        valid
            .then(|| Self(value.to_ascii_lowercase()))
            .ok_or(IconStorageError::InvalidKey)
    }

    /// Returns the filename that may be used only below the configured storage root.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Safe storage errors that never include a filesystem path or source bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconStorageError {
    /// A caller supplied a non-canonical or potentially unsafe key.
    InvalidKey,
    /// The configured storage cannot complete the requested operation.
    Unavailable,
}

impl fmt::Display for IconStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidKey => "station icon storage key is invalid",
            Self::Unavailable => "station icon storage is unavailable",
        })
    }
}

impl Error for IconStorageError {}

/// A validated and normalized ready-to-store WebP icon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedIcon {
    /// Lossless square WebP bytes safe for storage and HTTP delivery.
    pub bytes: Vec<u8>,
    /// SHA-256 content hash used as the storage key and strong ETag source.
    pub content_hash: [u8; 32],
    /// Final square artifact width.
    pub width: u32,
    /// Final square artifact height.
    pub height: u32,
}

/// Safe rejection class for untrusted icon sources and uploads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconValidationError {
    /// The source body is empty or exceeds the bounded byte limit.
    Size,
    /// The source URL is unusable (invalid, non-public, or unsuccessfully answered)
    /// or its bytes are not one of the accepted raster formats.
    Format,
    /// The source cannot be decoded as a safe raster image.
    Decode,
    /// The decoded source dimensions exceed the fixed processing limit.
    Dimensions,
}

impl fmt::Display for IconValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Size => "station icon size is invalid",
            Self::Format => "station icon format is invalid",
            Self::Decode => "station icon cannot be decoded",
            Self::Dimensions => "station icon dimensions are invalid",
        })
    }
}

impl Error for IconValidationError {}

/// Safe terminal result of a manual administrator icon change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualIconError {
    /// The supplied untrusted bytes failed the shared raster validation boundary.
    Validation(IconValidationError),
    /// The requested station does not exist, or has no manual override to remove.
    NotFound,
    /// Metadata or persistent artifact storage is temporarily unavailable.
    Unavailable,
}

impl fmt::Display for ManualIconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::NotFound => formatter.write_str("station icon target was not found"),
            Self::Unavailable => formatter.write_str("station icon storage is unavailable"),
        }
    }
}

impl Error for ManualIconError {}

/// Validates a raster source and converts it to the sole stored v1 format: square WebP.
pub fn prepare_icon(source: &[u8]) -> Result<PreparedIcon, IconValidationError> {
    if source.is_empty() || source.len() > MAX_SOURCE_BYTES {
        return Err(IconValidationError::Size);
    }
    let format = image::guess_format(source).map_err(|_| IconValidationError::Format)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Ico
    ) {
        return Err(IconValidationError::Format);
    }
    let image = image::load_from_memory_with_format(source, format)
        .map_err(|_| IconValidationError::Decode)?;
    let (width, height) = image.dimensions();
    if width == 0
        || height == 0
        || width > MAX_SOURCE_DIMENSION
        || height > MAX_SOURCE_DIMENSION
        || width.saturating_mul(height) > MAX_SOURCE_PIXELS
    {
        return Err(IconValidationError::Dimensions);
    }
    let resized = image
        .resize(ICON_DIMENSION, ICON_DIMENSION, FilterType::Lanczos3)
        .to_rgba8();
    let mut canvas = RgbaImage::new(ICON_DIMENSION, ICON_DIMENSION);
    let x = (ICON_DIMENSION - resized.width()) / 2;
    let y = (ICON_DIMENSION - resized.height()) / 2;
    image::imageops::overlay(&mut canvas, &resized, i64::from(x), i64::from(y));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut bytes, ImageFormat::WebP)
        .map_err(|_| IconValidationError::Decode)?;
    let bytes = bytes.into_inner();
    Ok(PreparedIcon {
        content_hash: Sha256::digest(&bytes).into(),
        bytes,
        width: ICON_DIMENSION,
        height: ICON_DIMENSION,
    })
}

/// Durable, safe counters displayed by the administrator while one import job runs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IconJobProgress {
    /// Stable job identifier returned immediately after the protected start request.
    pub id: Uuid,
    /// Terminal or active lifecycle state.
    pub status: String,
    /// Items selected when the job started.
    pub selected: i32,
    /// Items that reached a terminal per-item result.
    pub processed: i32,
    /// Artifacts successfully normalized and published.
    pub ready: i32,
    /// Items with no usable automatic source.
    pub missing: i32,
    /// Items eligible for a later explicit retry.
    pub retryable_error: i32,
    /// Items rejected permanently by validation.
    pub permanent_error: i32,
    /// Items deliberately left untouched, such as manual overrides.
    pub skipped: i32,
}

/// A ready artifact resolved from metadata and storage without exposing its source URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadyIcon {
    /// Prepared WebP bytes.
    pub bytes: Vec<u8>,
    /// SHA-256 content hash used by HTTP validators.
    pub content_hash: [u8; 32],
}

/// Minimal storage boundary shared by HTTP delivery, the importer, and manual upload.
#[async_trait]
pub trait IconStorage: Send + Sync {
    /// Reads a complete previously published artifact, or `None` when it is absent.
    async fn get(&self, key: &IconStorageKey) -> Result<Option<Vec<u8>>, IconStorageError>;

    /// Atomically publishes bytes for a canonical key without replacing a completed artifact.
    async fn put_atomic(&self, key: &IconStorageKey, bytes: &[u8]) -> Result<(), IconStorageError>;

    /// Checks whether a prepared artifact is present.
    async fn exists(&self, key: &IconStorageKey) -> Result<bool, IconStorageError>;

    /// Removes one prepared artifact after its metadata has been safely detached.
    async fn delete(&self, key: &IconStorageKey) -> Result<(), IconStorageError>;
}

/// Boundary for bounded external icon fetching, faked in tests without network access.
#[async_trait]
pub trait IconSourceFetcher: Send + Sync {
    /// Downloads and normalizes one validated external icon URL.
    async fn fetch_icon(&self, source: &str) -> Result<PreparedIcon, IconValidationError>;

    /// Resolves the ordered favicon candidate URLs for a station homepage, or why none
    /// can be fetched.
    async fn discover_homepage_icons(
        &self,
        homepage: &str,
    ) -> Result<Vec<String>, IconValidationError>;
}

pub(crate) fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
