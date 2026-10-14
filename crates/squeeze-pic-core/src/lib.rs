//! Offline image processing shared by the CLI and desktop worker.
#[cfg(target_os = "macos")]
mod apple_codec;
mod codecs;
mod jpeg;
mod jpeg_lossless;
mod metadata;
mod pipeline;
mod transform;
mod types;

pub use pipeline::{create_preview, inspect_file, process_file};
pub use transform::{output_dimensions, resolve_options};
pub use types::*;
