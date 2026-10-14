use crate::{metadata::Metadata, transform, types::*};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageEncoder, RgbaImage};
use std::io::Cursor;

pub fn detect(bytes: &[u8]) -> Result<Format> {
    if bytes.starts_with(b"icns") {
        return Ok(Format::Icns);
    }
    if bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a") {
        return Ok(Format::Jxl);
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        let brands = &bytes[8..bytes.len().min(64)];
        if brands.windows(4).any(|s| s == b"avif" || s == b"avis") {
            return Ok(Format::Avif);
        }
        if brands
            .windows(4)
            .any(|s| matches!(s, b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"))
        {
            return Ok(Format::Heif);
        }
    }
    if std::str::from_utf8(&bytes[..bytes.len().min(4096)])
        .ok()
        .is_some_and(|s| s.contains("<svg"))
    {
        return Ok(Format::Svg);
    }
    Ok(match image::guess_format(bytes)? {
        image::ImageFormat::Jpeg => Format::Jpeg,
        image::ImageFormat::Png => Format::Png,
        image::ImageFormat::WebP => Format::Webp,
        image::ImageFormat::Gif => Format::Gif,
        image::ImageFormat::Bmp => Format::Bmp,
        image::ImageFormat::Tiff => Format::Tiff,
        image::ImageFormat::Ico => Format::Ico,
        image::ImageFormat::Avif => Format::Avif,
        _ => return Err(err("unsupported", "Unsupported image format")),
    })
}
pub fn dimensions(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        Err(err("limits", "Image exceeds the 24 megapixel limit"))
    } else {
        Ok(())
    }
}
pub fn unpremultiply(pixels: &mut [u8]) {
    for p in pixels.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a > 0 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}
pub fn check_static(bytes: &[u8], format: Format) -> Result<()> {
    if format == Format::Tiff {
        let decoder =
            tiff::decoder::Decoder::new(Cursor::new(bytes)).map_err(|e| err("decode", e))?;
        if decoder.more_images() {
            return Err(err("animation", "Multi-page TIFF is not supported"));
        }
    }
    if format == Format::Gif {
        let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
        decoder.set_limits(image::Limits::default())?;
        let mut frames = decoder.into_frames();
        frames
            .next()
            .transpose()?
            .ok_or_else(|| err("decode", "Empty GIF"))?;
        if frames.next().transpose()?.is_some() {
            return Err(err(
                "animation",
                "Animated GIF is not supported; all frames must be preserved",
            ));
        }
    }
    Ok(())
}
pub fn decode_special(bytes: &[u8], format: Format) -> Result<(RgbaImage, u8)> {
    match format {
        Format::Heif | Format::Avif => {
            #[cfg(target_os = "macos")]
            {
                crate::apple_codec::decode(bytes)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(err(
                    "unsupported",
                    "HEIF/AVIF decoding currently requires macOS",
                ))
            }
        }
        Format::Jxl => {
            let header = jxl_oxide::JxlImage::builder()
                .alloc_tracker(jxl_oxide::AllocTracker::with_limit(256 * 1024 * 1024))
                .read(Cursor::new(bytes))
                .map_err(|e| err("decode", e))?;
            if header.image_header().metadata.animation.is_some() {
                return Err(err("animation", "Animated JPEG XL is not supported"));
            }
            dimensions(header.width(), header.height())?;
            if header.image_header().metadata.bit_depth.bits_per_sample() > 8 {
                return Err(err("bit_depth", "16-bit and HDR JPEG XL is not supported"));
            }
            let srgb = header.image_header().metadata.colour_encoding.is_srgb();
            drop(header);
            let mut decoder = jxl_oxide::integration::JxlDecoder::new(Cursor::new(bytes))?;
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(256 * 1024 * 1024);
            decoder.set_limits(limits)?;
            let icc = decoder.icc_profile()?;
            let mut pixels = DynamicImage::from_decoder(decoder)?.into_rgba8();
            if !srgb && let Some(icc) = icc {
                transform::convert_to_srgb(&mut pixels, &icc)?;
            }
            Ok((pixels, 1))
        }
        Format::Icns => {
            let family = icns::IconFamily::read(Cursor::new(bytes))?;
            let kind = family
                .available_icons()
                .into_iter()
                .max_by_key(|k| k.pixel_width())
                .ok_or_else(|| err("decode", "ICNS has no supported icons"))?;
            let icon = family
                .get_icon_with_type(kind)?
                .convert_to(icns::PixelFormat::RGBA);
            dimensions(icon.width(), icon.height())?;
            Ok((
                RgbaImage::from_raw(icon.width(), icon.height(), icon.data().to_vec()).unwrap(),
                1,
            ))
        }
        Format::Svg => {
            let external_image = std::sync::atomic::AtomicBool::new(false);
            let mut options = resvg::usvg::Options::default();
            options.image_href_resolver.resolve_string = Box::new(|_, _| {
                external_image.store(true, std::sync::atomic::Ordering::Relaxed);
                None
            });
            options.fontdb_mut().load_system_fonts();
            let tree =
                resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| err("decode", e))?;
            if external_image.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(err(
                    "svg_resources",
                    "SVG external images must be embedded before conversion",
                ));
            }
            let size = tree.size().to_int_size();
            dimensions(size.width(), size.height())?;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
                .ok_or_else(|| err("limits", "SVG raster allocation failed"))?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            let mut raw = pixmap.take();
            unpremultiply(&mut raw);
            Ok((
                RgbaImage::from_raw(size.width(), size.height(), raw).unwrap(),
                1,
            ))
        }
        _ => Err(err("unsupported", "Not a custom decoder")),
    }
}
fn fit_icon(pixels: &RgbaImage, edge: u32) -> RgbaImage {
    let fitted = DynamicImage::ImageRgba8(pixels.clone())
        .thumbnail(edge, edge)
        .into_rgba8();
    let mut canvas = RgbaImage::new(edge, edge);
    image::imageops::overlay(
        &mut canvas,
        &fitted,
        i64::from((edge - fitted.width()) / 2),
        i64::from((edge - fitted.height()) / 2),
    );
    canvas
}
pub fn encode(
    pixels: &RgbaImage,
    format: Format,
    resolved: &ResolvedOptions,
    options: &ProcessOptions,
    metadata: &Metadata,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, String)> {
    if metadata.icc.is_some() || metadata.exif.is_some() || metadata.xmp.is_some() {
        if options.preset == Preset::Lossless && metadata.icc.is_some() {
            return Err(err(
                "metadata",
                "Strict lossless requires an output that preserves the ICC profile",
            ));
        }
        warnings.push("This output codec omits camera metadata; pixels use sRGB.".into());
    }
    let mut output = Cursor::new(Vec::new());
    match format {
        Format::Avif => {
            let speed = match options.strategy {
                Strategy::Fast => 9,
                Strategy::Smart => 7,
                Strategy::Deep => 5,
            };
            image::codecs::avif::AvifEncoder::new_with_speed_quality(
                &mut output,
                speed,
                resolved.quality.unwrap_or(82),
            )
            .write_image(
                pixels,
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        Format::Jxl => {
            return Ok((
                encode_jxl(pixels, resolved, options)?,
                "jpeg-xl/libjxl".into(),
            ));
        }
        Format::Heif => {
            #[cfg(target_os = "macos")]
            {
                let rgb = transform::composite(pixels, options.background);
                let rgba: Vec<u8> = rgb
                    .chunks_exact(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect();
                return Ok((
                    crate::apple_codec::encode_heif(
                        &RgbaImage::from_raw(pixels.width(), pixels.height(), rgba).unwrap(),
                        resolved.quality.unwrap_or(82),
                    )?,
                    "heif/macos".into(),
                ));
            }
            #[cfg(not(target_os = "macos"))]
            return Err(err("unsupported", "HEIF output currently requires macOS"));
        }
        Format::Icns => {
            let edge = resolved.width;
            let pixels = fit_icon(pixels, edge);
            let icon =
                icns::Image::from_data(icns::PixelFormat::RGBA, edge, edge, pixels.into_raw())
                    .map_err(|e| err("encode", e))?;
            let mut family = icns::IconFamily::new();
            family.add_icon(&icon)?;
            family.write(&mut output)?;
        }
        Format::Ico => {
            DynamicImage::ImageRgba8(fit_icon(pixels, resolved.width))
                .write_to(&mut output, image::ImageFormat::Ico)?;
        }
        Format::Gif => {
            warnings.push("GIF is limited to 256 colors and one-bit transparency.".into());
            let speed = match options.strategy {
                Strategy::Fast => 20,
                Strategy::Smart => 10,
                Strategy::Deep => 1,
            };
            image::codecs::gif::GifEncoder::new_with_speed(&mut output, speed)
                .encode_frame(image::Frame::new(pixels.clone()))?;
        }
        Format::Tiff => {
            return encode_tiff(pixels, options.strategy);
        }
        Format::Bmp => {
            image::codecs::bmp::BmpEncoder::new(&mut output).write_image(
                pixels,
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        _ => return Err(err("unsupported", "No encoder for this format")),
    }
    Ok((
        output.into_inner(),
        format!("{}/encode", format.extension()),
    ))
}
fn encode_tiff(pixels: &RgbaImage, strategy: Strategy) -> Result<(Vec<u8>, String)> {
    use tiff::{
        encoder::{Compression, DeflateLevel, Predictor, TiffEncoder, colortype},
        tags::Tag,
    };
    let level = match strategy {
        Strategy::Fast => DeflateLevel::Fast,
        Strategy::Smart => DeflateLevel::Balanced,
        Strategy::Deep => DeflateLevel::Best,
    };
    let predictors: &[(Predictor, &str)] = if strategy == Strategy::Deep {
        &[
            (Predictor::Horizontal, "horizontal"),
            (Predictor::None, "none"),
        ]
    } else {
        &[(Predictor::Horizontal, "horizontal")]
    };
    let mut best: Option<(Vec<u8>, String)> = None;
    for &(predictor, name) in predictors {
        let mut output = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut output)
                .map_err(|e| err("encode", e))?
                .with_compression(Compression::Deflate(level))
                .with_predictor(predictor);
            let mut image = encoder
                .new_image::<colortype::RGBA8>(pixels.width(), pixels.height())
                .map_err(|e| err("encode", e))?;
            // The fourth channel is straight (unassociated) alpha; hidden RGB
            // must not be interpreted as premultiplied and discarded.
            image
                .encoder()
                .write_tag(Tag::ExtraSamples, &[2u16][..])
                .map_err(|e| err("encode", e))?;
            image
                .write_data(pixels.as_raw())
                .map_err(|e| err("encode", e))?;
        }
        let bytes = output.into_inner();
        if best
            .as_ref()
            .is_none_or(|(current, _)| bytes.len() < current.len())
        {
            best = Some((bytes, format!("tiff/deflate/{level:?}/{name}")));
        }
    }
    best.ok_or_else(|| err("encode", "No TIFF candidate"))
}
fn encode_jxl(
    pixels: &RgbaImage,
    resolved: &ResolvedOptions,
    options: &ProcessOptions,
) -> Result<Vec<u8>> {
    use jxl_sys::*;
    struct Encoder(*mut JxlEncoder);
    impl Drop for Encoder {
        fn drop(&mut self) {
            unsafe { JxlEncoderDestroy(self.0) }
        }
    }
    let encoder = Encoder(unsafe { JxlEncoderCreate(std::ptr::null()) });
    if encoder.0.is_null() {
        return Err(err("encode", "JPEG XL encoder allocation failed"));
    }
    let ok = |status: JxlEncoderStatus| {
        if status == JxlEncoderStatus::JXL_ENC_SUCCESS {
            Ok(())
        } else {
            Err(err("encode", "JPEG XL encoding failed"))
        }
    };
    unsafe {
        let mut info = JxlBasicInfo::default();
        JxlEncoderInitBasicInfo(&mut info);
        info.xsize = pixels.width();
        info.ysize = pixels.height();
        info.bits_per_sample = 8;
        info.num_color_channels = 3;
        info.num_extra_channels = 1;
        info.alpha_bits = 8;
        info.uses_original_profile = i32::from(resolved.lossless);
        ok(JxlEncoderSetBasicInfo(encoder.0, &info))?;
        let mut color = JxlColorEncoding::default();
        JxlColorEncodingSetToSRGB(&mut color, 0);
        ok(JxlEncoderSetColorEncoding(encoder.0, &color))?;
        let settings = JxlEncoderFrameSettingsCreate(encoder.0, std::ptr::null());
        ok(JxlEncoderSetFrameLossless(
            settings,
            i32::from(resolved.lossless),
        ))?;
        if resolved.lossless {
            ok(JxlEncoderFrameSettingsSetOption(
                settings,
                JxlEncoderFrameSettingId::JXL_ENC_FRAME_SETTING_KEEP_INVISIBLE,
                1,
            ))?;
        }
        if !resolved.lossless {
            let distance = (100. - f32::from(resolved.quality.unwrap_or(82))) * 0.09 + 0.1;
            ok(JxlEncoderSetFrameDistance(settings, distance))?;
        }
        let effort = match options.strategy {
            Strategy::Fast => 3,
            Strategy::Smart => 5,
            Strategy::Deep => 7,
        };
        ok(JxlEncoderFrameSettingsSetOption(
            settings,
            JxlEncoderFrameSettingId::JXL_ENC_FRAME_SETTING_EFFORT,
            effort,
        ))?;
        let layout = JxlPixelFormat {
            num_channels: 4,
            data_type: JxlDataType::JXL_TYPE_UINT8,
            endianness: JxlEndianness::JXL_NATIVE_ENDIAN,
            align: 0,
        };
        ok(JxlEncoderAddImageFrame(
            settings,
            &layout,
            pixels.as_ptr().cast(),
            pixels.len(),
        ))?;
        JxlEncoderCloseInput(encoder.0);
        let mut result = Vec::new();
        loop {
            let mut buffer = vec![0u8; 65536];
            let mut next = buffer.as_mut_ptr();
            let mut available = buffer.len();
            let status = JxlEncoderProcessOutput(encoder.0, &mut next, &mut available);
            result.extend_from_slice(&buffer[..buffer.len() - available]);
            if result.len() as u64 > MAX_INPUT {
                return Err(err("limits", "JPEG XL result exceeds size limit"));
            }
            if status == JxlEncoderStatus::JXL_ENC_SUCCESS {
                break;
            }
            if status != JxlEncoderStatus::JXL_ENC_NEED_MORE_OUTPUT {
                return Err(err("encode", "JPEG XL encoding failed"));
            }
        }
        Ok(result)
    }
}

pub fn jxl_srgb(bytes: &[u8]) -> Result<bool> {
    let header = jxl_oxide::JxlImage::builder()
        .alloc_tracker(jxl_oxide::AllocTracker::with_limit(256 * 1024 * 1024))
        .read(Cursor::new(bytes))
        .map_err(|e| err("decode", e))?;
    Ok(header.image_header().metadata.colour_encoding.is_srgb())
}
