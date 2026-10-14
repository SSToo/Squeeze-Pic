use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, PicError>;

#[derive(Debug, thiserror::Error, Serialize)]
#[error("{message}")]
pub struct PicError {
    pub code: String,
    pub message: String,
}
pub(crate) fn err(code: &str, message: impl ToString) -> PicError {
    PicError {
        code: code.into(),
        message: message.to_string(),
    }
}
impl From<std::io::Error> for PicError {
    fn from(e: std::io::Error) -> Self {
        err("io", e)
    }
}
impl From<image::ImageError> for PicError {
    fn from(e: image::ImageError) -> Self {
        err("decode", e)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
    Heif,
    Gif,
    Bmp,
    Tiff,
    Ico,
    Icns,
    Svg,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Jxl => "jxl",
            Self::Heif => "heic",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Ico => "ico",
            Self::Icns => "icns",
            Self::Svg => "svg",
        }
    }
    pub(crate) fn image(self) -> image::ImageFormat {
        match self {
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Png => image::ImageFormat::Png,
            Self::Webp => image::ImageFormat::WebP,
            Self::Avif => image::ImageFormat::Avif,
            Self::Gif => image::ImageFormat::Gif,
            Self::Bmp => image::ImageFormat::Bmp,
            Self::Tiff => image::ImageFormat::Tiff,
            Self::Ico => image::ImageFormat::Ico,
            Self::Jxl | Self::Heif | Self::Icns | Self::Svg => unreachable!("custom codec"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Lossless,
    High,
    #[default]
    Balanced,
    Small,
    Custom,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Fast,
    #[default]
    Smart,
    Deep,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetadataMode {
    #[default]
    Preserve,
    Strip,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResizeMode {
    #[default]
    Original,
    Percent,
    Longest,
    Width,
    Height,
    Fit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Resize {
    pub mode: ResizeMode,
    pub value: u32,
    pub width: u32,
    pub height: u32,
    pub allow_upscale: bool,
}
impl Default for Resize {
    fn default() -> Self {
        Self {
            mode: ResizeMode::Original,
            value: 1920,
            width: 1920,
            height: 1080,
            allow_upscale: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessOptions {
    pub format: Option<Format>,
    pub preset: Preset,
    pub strategy: Strategy,
    pub quality: u8,
    pub resize: Resize,
    pub metadata: MetadataMode,
    pub background: [u8; 3],
}
impl Default for ProcessOptions {
    fn default() -> Self {
        Self {
            format: None,
            preset: Preset::Balanced,
            strategy: Strategy::Smart,
            quality: 82,
            resize: Resize::default(),
            metadata: MetadataMode::Preserve,
            background: [255; 3],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub format: Format,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub has_alpha: bool,
    pub orientation: u8,
    pub has_icc: bool,
    pub has_exif: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedOptions {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub lossless: bool,
    pub quality: Option<u8>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessReport {
    pub source_sha256: String,
    pub outcome: String,
    pub output: Option<PathBuf>,
    pub original_bytes: u64,
    pub result_bytes: u64,
    pub reduction_percent: f64,
    pub input_format: Format,
    pub output_format: Format,
    pub before_dimensions: [u32; 2],
    pub after_dimensions: [u32; 2],
    pub elapsed_ms: u128,
    pub encoding: String,
    pub candidates: usize,
    pub quality: Option<u8>,
    pub warnings: Vec<String>,
}

pub const MAX_PIXELS: u64 = 24_000_000;
pub const MAX_INPUT: u64 = 64 * 1024 * 1024;
pub(crate) const MEMORY_BUDGET: u64 = 1024 * 1024 * 1024;
