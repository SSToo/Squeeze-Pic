use crate::types::*;
use image::{DynamicImage, Rgba, Rgba32FImage, RgbaImage, imageops::FilterType};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};

pub fn output_dimensions(width: u32, height: u32, resize: &Resize) -> Result<[u32; 2]> {
    if width == 0 || height == 0 {
        return Err(err("dimensions", "Invalid source dimensions"));
    }
    let positive = |n: u32| -> Result<f64> {
        if n == 0 || n > 100_000 {
            Err(err("dimensions", "Size must be between 1 and 100000"))
        } else {
            Ok(f64::from(n))
        }
    };
    let mut scale = match resize.mode {
        ResizeMode::Original => 1.,
        ResizeMode::Percent => {
            if resize.value > 400 {
                return Err(err("dimensions", "Scale must be between 1 and 400%"));
            }
            positive(resize.value)? / 100.
        }
        ResizeMode::Longest => positive(resize.value)? / f64::from(width.max(height)),
        ResizeMode::Width => positive(resize.value)? / f64::from(width),
        ResizeMode::Height => positive(resize.value)? / f64::from(height),
        ResizeMode::Fit => (positive(resize.width)? / f64::from(width))
            .min(positive(resize.height)? / f64::from(height)),
    };
    if !resize.allow_upscale {
        scale = scale.min(1.);
    }
    let dimensions = [
        (f64::from(width) * scale).round().max(1.) as u32,
        (f64::from(height) * scale).round().max(1.) as u32,
    ];
    if u64::from(dimensions[0]) * u64::from(dimensions[1]) > MAX_PIXELS {
        return Err(err("limits", "Output exceeds the 24 megapixel limit"));
    }
    Ok(dimensions)
}

pub fn resolve_options(info: &ImageInfo, options: &ProcessOptions) -> Result<ResolvedOptions> {
    let format = options.format.unwrap_or(info.format);
    let [mut width, mut height] =
        output_dimensions(info.display_width, info.display_height, &options.resize)?;
    if format == Format::Svg {
        return Err(err(
            "unsupported",
            "SVG can be imported and converted to raster formats; SVG export is not supported",
        ));
    }
    if format == Format::Ico || format == Format::Icns {
        let max = if format == Format::Ico { 256 } else { 1024 };
        let edge = width.max(height).min(max);
        let edge = if format == Format::Icns {
            [16, 32, 64, 128, 256, 512, 1024]
                .into_iter()
                .rev()
                .find(|n| *n <= edge)
                .unwrap_or(16)
        } else {
            edge
        };
        width = edge;
        height = edge;
    }
    if format == Format::Webp && (width > 16383 || height > 16383) {
        return Err(err(
            "dimensions",
            "WebP dimensions must not exceed 16383 pixels",
        ));
    }
    if options.preset == Preset::Custom && !(1..=100).contains(&options.quality) {
        return Err(err("quality", "Quality must be between 1 and 100"));
    }
    let strict = options.preset == Preset::Lossless;
    if strict && options.resize.mode != ResizeMode::Original {
        return Err(err(
            "lossless_resize",
            "Strict lossless cannot resize pixels; choose another quality preset",
        ));
    }
    if strict && format == Format::Jpeg && info.format != Format::Jpeg {
        return Err(err(
            "lossless_jpeg",
            "JPEG lossless optimization requires an existing JPEG",
        ));
    }
    if strict
        && matches!(
            format,
            Format::Avif | Format::Heif | Format::Gif | Format::Ico | Format::Icns
        )
    {
        return Err(err(
            "lossless_format",
            "This output requires a lossy or icon conversion preset",
        ));
    }
    let lossless = strict
        || matches!(
            format,
            Format::Png | Format::Bmp | Format::Tiff | Format::Ico | Format::Icns
        );
    let quality = if lossless {
        None
    } else {
        Some(match (format, options.preset) {
            (_, Preset::Custom) => options.quality,
            (Format::Webp, Preset::High) => 90,
            (Format::Webp, Preset::Small) => 60,
            (Format::Webp, _) => 80,
            (_, Preset::High) => 92,
            (_, Preset::Small) => 65,
            _ => 82,
        })
    };
    let mut warnings = Vec::new();
    if format == Format::Png && !strict {
        warnings.push("PNG uses lossless encoding; quality presets do not quantize colors.".into());
    }
    if info.has_alpha && matches!(format, Format::Jpeg | Format::Heif) {
        warnings.push("Transparency is composited onto the selected background.".into());
    }
    Ok(ResolvedOptions {
        format,
        width,
        height,
        lossless,
        quality,
        warnings,
    })
}

pub(crate) fn convert_to_srgb(pixels: &mut RgbaImage, icc: &[u8]) -> Result<Vec<u8>> {
    let source = ColorProfile::new_from_slice(icc).map_err(|e| err("color", e))?;
    if source.color_space != DataColorSpace::Rgb {
        return Err(err(
            "color",
            "Only RGB ICC profiles are supported in this version",
        ));
    }
    let dest = ColorProfile::new_srgb();
    let transform = source
        .create_transform_8bit(
            Layout::Rgba,
            &dest,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|e| err("color", e))?;
    // Transform one row at a time rather than duplicate the entire image.
    let stride = pixels.width() as usize * 4;
    let mut row = vec![0; stride];
    for source in pixels.as_mut().chunks_exact_mut(stride) {
        transform
            .transform(source, &mut row)
            .map_err(|e| err("color", e))?;
        // Alpha is coverage, not a color component.
        for (dst, old) in row.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
            dst[3] = old[3];
        }
        source.copy_from_slice(&row);
    }
    dest.encode().map_err(|e| err("color", e))
}
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_table() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|v| linear(v as f32 / 255.)))
}
fn encoded(v: f32) -> u8 {
    let v = v.clamp(0., 1.);
    let v = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (v * 255.).round() as u8
}

pub(crate) fn resize_rgba(pixels: RgbaImage, width: u32, height: u32) -> RgbaImage {
    if pixels.dimensions() == (width, height) {
        return pixels;
    }
    let mut work = Rgba32FImage::new(pixels.width(), pixels.height());
    let linear = linear_table();
    for (dst, src) in work.pixels_mut().zip(pixels.pixels()) {
        let a = f32::from(src[3]) / 255.;
        *dst = Rgba([
            linear[src[0] as usize] * a,
            linear[src[1] as usize] * a,
            linear[src[2] as usize] * a,
            a,
        ]);
    }
    drop(pixels);
    let work = image::imageops::resize(&work, width, height, FilterType::Lanczos3);
    RgbaImage::from_fn(width, height, |x, y| {
        let p = work.get_pixel(x, y);
        let a = p[3].clamp(0., 1.);
        if a <= 0.00001 {
            Rgba([0; 4])
        } else {
            Rgba([
                encoded(p[0] / a),
                encoded(p[1] / a),
                encoded(p[2] / a),
                (a * 255.).round() as u8,
            ])
        }
    })
}

pub(crate) fn composite(pixels: &RgbaImage, bg: [u8; 3]) -> Vec<u8> {
    let mut rgb = vec![0; pixels.len() / 4 * 3];
    let linear = linear_table();
    let background = bg.map(|c| linear[c as usize]);
    for (dst, p) in rgb.chunks_exact_mut(3).zip(pixels.pixels()) {
        match p[3] {
            255 => dst.copy_from_slice(&p.0[..3]),
            0 => dst.copy_from_slice(&bg),
            alpha => {
                let a = f32::from(alpha) / 255.;
                for c in 0..3 {
                    dst[c] = encoded(linear[p[c] as usize] * a + background[c] * (1. - a));
                }
            }
        }
    }
    rgb
}
pub(crate) fn oriented(pixels: RgbaImage, orientation: u8) -> RgbaImage {
    let mut image = DynamicImage::ImageRgba8(pixels);
    if let Some(o) = image::metadata::Orientation::from_exif(orientation) {
        image.apply_orientation(o);
    }
    image.into_rgba8()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_fast_paths_and_lookup_keep_the_srgb_reference_result() {
        // All source channel/alpha pairs, including opaque and invisible pixels.
        let pixels = RgbaImage::from_fn(256, 256, |value, alpha| {
            Rgba([
                value as u8,
                (255 - value) as u8,
                (value ^ 0x55) as u8,
                alpha as u8,
            ])
        });
        for bg in [[0, 255, 0], [1, 127, 254], [255; 3]] {
            let reference: Vec<u8> = pixels
                .pixels()
                .flat_map(|p| {
                    let a = f32::from(p[3]) / 255.;
                    (0..3).map(move |c| {
                        encoded(
                            linear(f32::from(p[c]) / 255.) * a
                                + linear(f32::from(bg[c]) / 255.) * (1. - a),
                        )
                    })
                })
                .collect();
            assert_eq!(composite(&pixels, bg), reference);
        }
    }
}
