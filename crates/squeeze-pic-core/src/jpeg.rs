use mozjpeg::{ColorSpace, Compress, ScanMode};

#[cfg(test)]
pub(crate) fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    gray: bool,
    quality: u8,
) -> std::result::Result<Vec<u8>, String> {
    encode_profile(pixels, width, height, gray, quality, false, quality >= 90)
}
pub(crate) fn encode_profile(
    pixels: &[u8],
    width: u32,
    height: u32,
    gray: bool,
    quality: u8,
    fast: bool,
    full_chroma: bool,
) -> std::result::Result<Vec<u8>, String> {
    let channels = if gray { 1 } else { 3 };
    if width == 0
        || height == 0
        || width > 65_500
        || height > 65_500
        || !(1..=100).contains(&quality)
        || (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(channels))
            != Some(pixels.len())
    {
        return Err("Invalid JPEG dimensions, quality or pixel buffer".into());
    }
    std::panic::catch_unwind(|| -> std::io::Result<Vec<u8>> {
        let mut encoder = Compress::new(if gray {
            ColorSpace::JCS_GRAYSCALE
        } else {
            ColorSpace::JCS_RGB
        });
        if fast {
            encoder.set_fastest_defaults();
        } else {
            encoder.set_scan_optimization_mode(ScanMode::Auto);
        }
        encoder.set_size(width as usize, height as usize);
        encoder.set_quality(f32::from(quality));
        if !gray {
            let sampling = if full_chroma { (1, 1) } else { (2, 2) };
            encoder.set_chroma_sampling_pixel_sizes(sampling, sampling);
        }
        let mut encoder = encoder.start_compress(Vec::new())?;
        encoder.write_scanlines(pixels)?;
        encoder.finish()
    })
    .map_err(|_| "JPEG encoder failed".to_string())?
    .map_err(|e| e.to_string())
}
