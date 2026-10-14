use crate::{
    codecs, jpeg, jpeg_lossless,
    metadata::{self, Metadata},
    transform,
    types::*,
};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, RgbaImage};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

struct Source<'a> {
    bytes: Cow<'a, [u8]>,
    info: ImageInfo,
    metadata: Metadata,
    special_color: bool,
    pixels: Option<RgbaImage>,
}
fn open_input(path: &Path) -> Result<File> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(err("input", "Input must be a regular file"));
    }
    if metadata.len() > MAX_INPUT {
        return Err(err("limits", "Input exceeds the 64 MiB limit"));
    }
    Ok(file)
}
fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = open_input(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(err("limits", "Input grew beyond the 64 MiB limit"));
    }
    Ok(bytes)
}
fn hash_file(path: &Path) -> Result<sha2::digest::Output<Sha256>> {
    let mut file = open_input(path)?.take(MAX_INPUT + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_INPUT {
            return Err(err("limits", "Input grew beyond the 64 MiB limit"));
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize())
}
fn reader(bytes: &[u8], format: Format) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format.image());
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits.max_image_width = Some(65_500);
    limits.max_image_height = Some(65_500);
    reader.limits(limits);
    reader
}
fn source(bytes: Cow<'_, [u8]>) -> Result<Source<'_>> {
    let format = codecs::detect(&bytes)?;
    if matches!(
        format,
        Format::Avif | Format::Heif | Format::Jxl | Format::Svg | Format::Icns
    ) {
        let (pixels, orientation) = codecs::decode_special(&bytes, format)?;
        let (width, height) = pixels.dimensions();
        let swapped = orientation >= 5;
        return Ok(Source {
            info: ImageInfo {
                format,
                bytes: bytes.len() as u64,
                width,
                height,
                display_width: if swapped { height } else { width },
                display_height: if swapped { width } else { height },
                has_alpha: pixels.pixels().any(|p| p[3] != 255),
                orientation,
                has_icc: false,
                has_exif: orientation != 1,
            },
            bytes,
            metadata: Metadata::default(),
            special_color: false,
            pixels: Some(pixels),
        });
    }
    codecs::check_static(&bytes, format)?;
    if format == Format::Jpeg
        && jpeg_lossless::components(&bytes).is_none_or(|n| !matches!(n, 1 | 3))
    {
        return Err(err(
            "color",
            "CMYK and nonstandard JPEG are not supported in this version",
        ));
    }
    let mut metadata = Metadata::default();
    let mut special_color = false;
    if format == Format::Png {
        let chunks = metadata::png_chunks(&bytes)?;
        if chunks
            .iter()
            .any(|(tag, data)| tag == b"cICP" && *data != [1, 13, 0, 1])
        {
            return Err(err(
                "color",
                "HDR and non-sRGB CICP configurations are not supported yet",
            ));
        }
        if chunks.iter().any(|(tag, _)| tag == b"acTL") {
            return Err(err(
                "animation",
                "Animated PNG is not supported yet; the first frame will not be silently extracted",
            ));
        }
        let tagged_srgb = chunks.iter().any(|(tag, data)| {
            tag == b"sRGB" || tag == b"iCCP" || (tag == b"cICP" && *data == [1, 13, 0, 1])
        });
        special_color = !tagged_srgb
            && chunks.iter().any(|(tag, data)| {
                tag == b"cHRM"
                    || tag == b"cICP"
                    || (tag == b"gAMA" && *data != 45_455u32.to_be_bytes())
            });
        for (tag, data) in chunks {
            if matches!(&tag, b"tEXt" | b"zTXt" | b"iTXt" | b"pHYs") {
                if tag == *b"iTXt" && data.starts_with(b"XML:com.adobe.xmp\0") {
                    continue;
                }
                metadata.png_text.push((tag, data.to_vec()));
            }
        }
    }
    if format == Format::Webp
        && metadata::riff_chunks(&bytes)?
            .iter()
            .any(|(tag, _)| matches!(tag, b"ANIM" | b"ANMF"))
    {
        return Err(err("animation", "Animated WebP is not supported yet"));
    }
    if format == Format::Jpeg {
        metadata.other_metadata = jpeg_has_other_metadata(&bytes);
    }
    let mut decoder = reader(&bytes, format).into_decoder()?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PIXELS || pixels * 40 + bytes.len() as u64 * 3 > MEMORY_BUDGET {
        return Err(err(
            "limits",
            "Image exceeds the 24 megapixel or estimated 1 GiB working budget",
        ));
    }
    if decoder.color_type().bits_per_pixel() / u16::from(decoder.color_type().channel_count()) > 8 {
        return Err(err(
            "bit_depth",
            "16-bit and HDR images are not supported yet; no silent down-conversion is performed",
        ));
    }
    metadata.icc = decoder.icc_profile()?;
    metadata.exif = decoder.exif_metadata()?;
    metadata.xmp = decoder.xmp_metadata()?;
    for value in [&metadata.icc, &metadata.exif, &metadata.xmp] {
        if value.as_ref().is_some_and(|b| b.len() > 4 * 1024 * 1024) {
            return Err(err("limits", "Metadata exceeds 4 MiB per block"));
        }
    }
    let orientation = metadata::orientation(metadata.exif.as_deref());
    if format == Format::Jpeg {
        jpeg_lossless::validate(&bytes, [width, height], MAX_PIXELS, 256 * 1024 * 1024)
            .map_err(|e| err("decode", e))?;
    }
    let swapped = orientation >= 5;
    let info = ImageInfo {
        format,
        bytes: bytes.len() as u64,
        width,
        height,
        display_width: if swapped { height } else { width },
        display_height: if swapped { width } else { height },
        has_alpha: decoder.color_type().has_alpha(),
        orientation,
        has_icc: metadata.icc.is_some(),
        has_exif: metadata.exif.is_some(),
    };
    drop(decoder);
    Ok(Source {
        bytes,
        info,
        metadata,
        special_color,
        pixels: None,
    })
}
fn jpeg_has_other_metadata(bytes: &[u8]) -> bool {
    let mut p = 2;
    while let Some(header) = bytes.get(p..p + 4) {
        if header[0] != 255 || matches!(header[1], 0xda | 0xd9) {
            break;
        }
        let len = u16::from_be_bytes([header[2], header[3]]) as usize;
        if len < 2 {
            break;
        }
        if header[1] == 0xfe || (0xe3..=0xef).contains(&header[1]) {
            return true;
        }
        p += 2 + len;
    }
    false
}
fn decode(source: &Source<'_>) -> Result<RgbaImage> {
    if let Some(pixels) = &source.pixels {
        return Ok(pixels.clone());
    }
    Ok(reader(&source.bytes, source.info.format)
        .decode()?
        .into_rgba8())
}
pub fn inspect_file(path: impl AsRef<Path>) -> Result<ImageInfo> {
    Ok(source(read_bytes(path.as_ref())?.into())?.info)
}
fn check(cancel: &AtomicBool, started: Instant) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(err("cancelled", "Task cancelled"));
    }
    if started.elapsed() > Duration::from_secs(120) {
        return Err(err("timeout", "Task exceeded 120 seconds"));
    }
    Ok(())
}
fn png_options(strategy: Strategy, allow_grayscale: bool) -> oxipng::Options {
    let level = match strategy {
        Strategy::Fast => 1,
        Strategy::Smart => 3,
        Strategy::Deep => 5,
    };
    let mut options = oxipng::Options::from_preset(level);
    // Keep exact hidden RGB. Palette/bit-depth reductions only change storage,
    // and are checked against the decoded original before an output is committed.
    options.optimize_alpha = false;
    // A grayscale PNG must not carry a preserved RGB ICC profile.
    options.grayscale_reduction = allow_grayscale;
    options.timeout = Some(Duration::from_secs(20));
    options.max_decompressed_size = Some(128 * 1024 * 1024);
    options
}
fn optimize_png(bytes: &[u8], strategy: Strategy, allow_grayscale: bool) -> Result<Vec<u8>> {
    let options = png_options(strategy, allow_grayscale);
    oxipng::optimize_from_memory(bytes, &options).map_err(|e| err("encode", e))
}
fn encode_png(
    pixels: RgbaImage,
    metadata: &Metadata,
    strategy: Strategy,
) -> Result<(Vec<u8>, &'static str)> {
    let (width, height) = pixels.dimensions();
    // The raw optimizer regresses on small low-color graphics in Fast mode.
    // Keep one direct encode there; use the raw path for full-color images.
    if strategy == Strategy::Fast && screenshot_like(&pixels) {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        if let Some(icc) = &metadata.icc {
            encoder
                .set_icc_profile(icc.clone())
                .map_err(|e| err("metadata", e))?;
        }
        if let Some(exif) = &metadata.exif {
            encoder
                .set_exif_metadata(exif.clone())
                .map_err(|e| err("metadata", e))?;
        }
        encoder.write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)?;
        return Ok((metadata::attach_png(bytes, metadata), "png/single-pass"));
    }
    let mut data = pixels.into_raw();
    let mut color_type = oxipng::ColorType::RGBA;
    let mut options = png_options(strategy, metadata.icc.is_none());
    if strategy == Strategy::Fast {
        // Compact an unused opaque alpha channel in place; do not spend Fast
        // mode searching palettes. Partial/invisible alpha retains exact RGB.
        if data.chunks_exact(4).all(|p| p[3] == 255) {
            for i in 0..data.len() / 4 {
                data.copy_within(i * 4..i * 4 + 3, i * 3);
            }
            data.truncate(width as usize * height as usize * 3);
            color_type = oxipng::ColorType::RGB {
                transparent_color: None,
            };
        }
        options.color_type_reduction = false;
        options.deflater = oxipng::Deflater::Libdeflater { compression: 5 };
    }
    // Transfer the pixel buffer straight to the optimizer. Avoid a preliminary
    // high-effort PNG encode, decompression and a second full compression.
    let mut raw = oxipng::RawImage::new(width, height, color_type, oxipng::BitDepth::Eight, data)
        .map_err(|e| err("encode", e))?;
    if let Some(icc) = &metadata.icc {
        raw.add_icc_profile(icc);
    }
    if let Some(exif) = &metadata.exif {
        raw.add_png_chunk(
            *b"eXIf",
            exif.strip_prefix(b"Exif\0\0").unwrap_or(exif).to_vec(),
        );
    }
    for (tag, data) in metadata::png_extra_chunks(metadata) {
        raw.add_png_chunk(tag, data);
    }
    raw.create_optimized_png(&options)
        .map(|bytes| (bytes, "png/raw-optimized"))
        .map_err(|e| err("encode", e))
}

pub fn process_file(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: &ProcessOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str),
) -> Result<ProcessReport> {
    let started = Instant::now();
    check(cancel, started)?;
    progress("reading");
    let input = input.as_ref();
    let output = output.as_ref();
    let mut source = source(read_bytes(input)?.into())?;
    let hash = Sha256::digest(&source.bytes);
    let resolved = transform::resolve_options(&source.info, options)?;
    if Format::extension(resolved.format)
        != output
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
        && !(resolved.format == Format::Jpeg
            && output
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("jpeg")))
    {
        return Err(err(
            "output_format",
            "Output extension must match the selected format",
        ));
    }
    if output.exists() {
        return Err(err(
            "exists",
            "Output already exists; existing files are never overwritten",
        ));
    }
    if options.preset == Preset::Lossless
        && (matches!(source.info.format, Format::Heif | Format::Avif)
            || (source.info.format == Format::Jxl && !codecs::jxl_srgb(&source.bytes)?))
    {
        return Err(err(
            "lossless_format",
            "Strict lossless is unavailable for this system-decoded or non-sRGB input",
        ));
    }
    if source.info.format == resolved.format
        && matches!(resolved.format, Format::Ico | Format::Icns)
    {
        let count = if resolved.format == Format::Ico {
            source
                .bytes
                .get(4..6)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
                .unwrap_or(0)
        } else {
            icns::IconFamily::read(Cursor::new(&source.bytes))?
                .available_icons()
                .len()
        };
        if count > 1 {
            return Err(err(
                "icon_family",
                "Multi-size icons can be converted from their largest image; same-format compression must preserve every size and is not supported yet",
            ));
        }
    }
    let mut warnings = resolved.warnings.clone();
    if matches!(
        source.info.format,
        Format::Heif | Format::Avif | Format::Jxl
    ) {
        warnings.push(
            "Extended-format input is decoded to sRGB; camera metadata is not retained.".into(),
        );
    }
    let encoding: String;
    let candidates: usize;
    let mut png_expectation: Option<([u8; 32], Option<Vec<u8>>)> = None;
    let same_png = source.info.format == Format::Png
        && resolved.format == Format::Png
        && options.resize.mode == ResizeMode::Original
        && options.metadata == MetadataMode::Preserve;
    let strict_jpeg = options.preset == Preset::Lossless && resolved.format == Format::Jpeg;
    progress("encoding");
    check(cancel, started)?;
    let bytes = if strict_jpeg {
        if options.metadata != MetadataMode::Preserve {
            return Err(err(
                "lossless_metadata",
                "JPEG coefficient optimization currently requires preserving metadata",
            ));
        }
        let variants = jpeg_lossless::optimize(
            &source.bytes,
            [source.info.width, source.info.height],
            MAX_PIXELS,
            256 * 1024 * 1024,
            options.strategy == Strategy::Fast,
        )
        .map_err(|e| err("encode", e))?;
        candidates = variants.len();
        let (name, result) = variants
            .into_iter()
            .min_by_key(|(_, bytes)| bytes.len())
            .ok_or_else(|| err("encode", "No JPEG candidate"))?;
        encoding = name.into();
        result
    } else if same_png {
        encoding = "png/lossless-optimize".into();
        candidates = 1;
        optimize_png(
            &source.bytes,
            options.strategy,
            source.metadata.icc.is_none() && !source.special_color,
        )?
    } else {
        if source.special_color {
            return Err(err(
                "color",
                "This PNG uses color metadata that is not yet supported for pixel conversion; use original PNG optimization",
            ));
        }
        let mut pixels = if options.preset != Preset::Lossless {
            // Native/JXL/SVG decoders already produced owned pixels during
            // inspection. Non-strict processing can consume that buffer.
            source.pixels.take().map_or_else(|| decode(&source), Ok)?
        } else {
            decode(&source)?
        };
        let mut metadata = source.metadata.clone();
        if options.preset == Preset::Lossless
            && resolved.format != source.info.format
            && let Some(icc) = &metadata.icc
        {
            let profile = moxcms::ColorProfile::new_from_slice(icc).map_err(|e| err("color", e))?;
            if profile.color_space != moxcms::DataColorSpace::Rgb {
                return Err(err(
                    "color",
                    "Strict cross-format conversion currently requires an RGB ICC profile",
                ));
            }
        }
        if options.preset != Preset::Lossless {
            if let Some(icc) = &metadata.icc {
                metadata.icc = Some(transform::convert_to_srgb(&mut pixels, icc)?);
                warnings.push("The embedded RGB ICC profile was converted to sRGB.".into());
            }
            pixels = transform::oriented(pixels, source.info.orientation);
            progress("resizing");
            check(cancel, started)?;
            if matches!(resolved.format, Format::Ico | Format::Icns) {
                let scale = resolved.width as f64 / pixels.width().max(pixels.height()) as f64;
                let width = (pixels.width() as f64 * scale).round().max(1.) as u32;
                let height = (pixels.height() as f64 * scale).round().max(1.) as u32;
                pixels = transform::resize_rgba(pixels, width, height);
            } else {
                pixels = transform::resize_rgba(pixels, resolved.width, resolved.height);
            }
            if source.info.orientation != 1
                || pixels.dimensions() != (source.info.width, source.info.height)
            {
                if let Some(exif) = &mut metadata.exif {
                    metadata::normalize_exif(exif, resolved.width, resolved.height)?;
                }
                if metadata.xmp.take().is_some() {
                    warnings.push("XMP was omitted after rotation/resizing to avoid stale dimensions and orientation.".into());
                }
            }
        }
        if options.metadata == MetadataMode::Strip {
            if options.preset == Preset::Lossless && source.info.orientation != 1 {
                return Err(err(
                    "lossless_metadata",
                    "Removing orientation metadata requires a pixel rotation; choose another quality preset",
                ));
            }
            metadata.exif = None;
            metadata.xmp = None;
            metadata.png_text.clear();
        } else {
            if source.metadata.other_metadata {
                warnings.push("Unmapped JPEG application/comment metadata was omitted during pixel re-encoding.".into());
            }
            if resolved.format != Format::Png && !metadata.png_text.is_empty() {
                metadata.png_text.clear();
                warnings.push("PNG text and physical-resolution chunks cannot be carried into this output format.".into());
            }
        }
        progress("encoding");
        check(cancel, started)?;
        let mut best: Option<(String, Vec<u8>)> = None;
        let mut candidate_count = 0;
        // Keep only the current winner while searching, rather than retaining
        // several complete encodings (up to 64 MiB each) for the entire search.
        let mut consider = |candidate: (String, Vec<u8>)| {
            candidate_count += 1;
            if best
                .as_ref()
                .is_none_or(|(_, bytes)| candidate.1.len() < bytes.len())
            {
                best = Some(candidate);
            }
        };
        match resolved.format {
            Format::Png => {
                // Verify the transformed pixels and profile even for ordinary
                // conversions/resizes, without retaining an extra RGBA buffer.
                png_expectation =
                    Some((Sha256::digest(pixels.as_raw()).into(), metadata.icc.clone()));
                let (encoded, name) = encode_png(pixels, &metadata, options.strategy)?;
                consider((name.into(), encoded));
            }
            Format::Jpeg => {
                let rgb = transform::composite(&pixels, options.background);
                // Exact channel equality lets grayscale photographs use one component.
                let gray = metadata.icc.is_none()
                    && rgb.chunks_exact(3).all(|p| p[0] == p[1] && p[1] == p[2]);
                let gray_pixels = if gray {
                    rgb.chunks_exact(3).map(|p| p[0]).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let raw = if gray { &gray_pixels } else { &rgb };
                let quality = resolved.quality.unwrap();
                let full_chroma = quality >= 90
                    || (options.strategy != Strategy::Fast && screenshot_like(&pixels));
                let profiles = if options.strategy == Strategy::Deep {
                    vec![true, false]
                } else {
                    vec![options.strategy == Strategy::Fast]
                };
                for fast in profiles {
                    check(cancel, started)?;
                    let encoded = jpeg::encode_profile(
                        raw,
                        pixels.width(),
                        pixels.height(),
                        gray,
                        quality,
                        fast,
                        full_chroma,
                    )
                    .map_err(|e| err("encode", e))?;
                    consider((
                        format!("jpeg/mozjpeg/{}", if fast { "fast" } else { "optimized" }),
                        metadata::attach_jpeg(encoded, &metadata)?,
                    ));
                }
            }
            format if !matches!(format, Format::Webp) => {
                let (encoded, name) = codecs::encode(
                    &pixels,
                    format,
                    &resolved,
                    options,
                    &metadata,
                    &mut warnings,
                )?;
                consider((name, encoded));
            }
            Format::Webp => {
                let methods: &[i32] = match options.strategy {
                    Strategy::Fast => &[1],
                    Strategy::Smart => &[4],
                    Strategy::Deep => &[1, 4, 6],
                };
                let sharp_yuv = !resolved.lossless
                    && options.strategy != Strategy::Fast
                    && screenshot_like(&pixels);
                let has_alpha = pixels.pixels().any(|p| p[3] != 255);
                for &method in methods {
                    check(cancel, started)?;
                    let mut config =
                        webp::WebPConfig::new().map_err(|e| err("encode", format!("{e:?}")))?;
                    config.lossless = resolved.lossless.into();
                    config.quality = f32::from(resolved.quality.unwrap_or(90));
                    config.method = method;
                    config.exact = 1;
                    config.alpha_quality = 100;
                    config.thread_level = 0;
                    config.use_sharp_yuv = sharp_yuv.into();
                    let encoded =
                        webp::Encoder::from_rgba(&pixels, pixels.width(), pixels.height())
                            .encode_advanced(&config)
                            .map_err(|e| err("encode", format!("{e:?}")))?
                            .to_vec();
                    consider((
                        format!(
                            "webp/{}/method-{method}",
                            if resolved.lossless {
                                "lossless"
                            } else {
                                "lossy"
                            }
                        ),
                        metadata::attach_webp(
                            encoded,
                            &metadata,
                            pixels.width(),
                            pixels.height(),
                            has_alpha,
                        )?,
                    ));
                }
            }
            _ => unreachable!(),
        }
        candidates = candidate_count;
        let (name, result) = best.ok_or_else(|| err("encode", "No encoding candidate"))?;
        encoding = name;
        result
    };
    if bytes.len() as u64 > MAX_INPUT {
        return Err(err("limits", "Encoded result exceeds the 64 MiB limit"));
    }
    progress("validating");
    check(cancel, started)?;
    let mut validated = source_from_candidate(&bytes)?;
    if validated.info.format != resolved.format
        || [validated.info.display_width, validated.info.display_height]
            != [resolved.width, resolved.height]
    {
        return Err(err(
            "validation",
            "Output format or visible dimensions mismatch",
        ));
    }
    // Always decode the complete result before committing it.
    let result_pixels = validated
        .pixels
        .take()
        .map_or_else(|| decode(&validated), Ok)?;
    if let Some((expected_pixels, expected_icc)) = png_expectation {
        let actual_pixels: [u8; 32] = Sha256::digest(result_pixels.as_raw()).into();
        if actual_pixels != expected_pixels || validated.metadata.icc != expected_icc {
            return Err(err(
                "validation",
                "PNG transformed pixel or ICC verification failed",
            ));
        }
    }
    if options.preset == Preset::Lossless || same_png {
        let original_pixels = decode(&source)?;
        if original_pixels != result_pixels {
            return Err(err(
                "validation",
                "Strict lossless pixel verification failed",
            ));
        }
        if validated.metadata.icc != source.metadata.icc {
            return Err(err("validation", "Strict lossless ICC verification failed"));
        }
    }
    drop(result_pixels);
    let must_export = resolved.format != source.info.format
        || options.resize.mode != ResizeMode::Original
        || options.metadata != MetadataMode::Preserve;
    let unchanged = !must_export && bytes.len() >= source.bytes.len();
    let size = if unchanged {
        source.bytes.len()
    } else {
        bytes.len()
    } as u64;
    let mut report = ProcessReport {
        source_sha256: format!("{hash:x}"),
        outcome: if unchanged { "unchanged" } else { "saved" }.into(),
        output: None,
        original_bytes: source.info.bytes,
        result_bytes: size,
        reduction_percent: (1. - size as f64 / source.info.bytes as f64) * 100.,
        input_format: source.info.format,
        output_format: resolved.format,
        before_dimensions: [source.info.display_width, source.info.display_height],
        after_dimensions: [resolved.width, resolved.height],
        elapsed_ms: 0,
        encoding,
        candidates,
        quality: resolved.quality,
        warnings,
    };
    check(cancel, started)?;
    if hash_file(input)? != hash {
        return Err(err(
            "input_changed",
            "Original changed during processing; output was not committed",
        ));
    }
    if !unchanged {
        progress("saving");
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        check(cancel, started)?;
        if hash_file(input)? != hash {
            return Err(err(
                "input_changed",
                "Original changed before commit; output was not committed",
            ));
        }
        temporary
            .persist_noclobber(output)
            .map_err(|e| err("commit", e.error))?;
        report.output = Some(output.to_path_buf());
    }
    report.elapsed_ms = started.elapsed().as_millis();
    progress("completed");
    Ok(report)
}

fn source_from_candidate(bytes: &[u8]) -> Result<Source<'_>> {
    // The encoded result outlives validation, so its complete buffer need not be copied.
    source(Cow::Borrowed(bytes))
}
fn screenshot_like(pixels: &RgbaImage) -> bool {
    let step = (pixels.len() / 4 / 2048).max(1);
    let mut colors = std::collections::HashSet::new();
    for p in pixels.pixels().step_by(step) {
        colors.insert([p[0] >> 3, p[1] >> 3, p[2] >> 3]);
        if colors.len() > 180 {
            return false;
        }
    }
    true
}

pub fn create_preview(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    max_edge: u32,
) -> Result<ImageInfo> {
    if !(32..=2048).contains(&max_edge) {
        return Err(err(
            "dimensions",
            "Preview edge must be between 32 and 2048",
        ));
    }
    let mut source = source(read_bytes(input.as_ref())?.into())?;
    if source.special_color {
        return Err(err(
            "color",
            "Preview for this PNG color configuration is not yet supported",
        ));
    }
    let mut pixels = source.pixels.take().map_or_else(|| decode(&source), Ok)?;
    if let Some(icc) = &source.metadata.icc {
        transform::convert_to_srgb(&mut pixels, icc)?;
    }
    let image = DynamicImage::ImageRgba8(transform::oriented(pixels, source.info.orientation));
    let image = image.thumbnail(max_edge, max_edge);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    image.write_to(&mut file, image::ImageFormat::Png)?;
    Ok(source.info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_encoders_preserve_hidden_rgb_icc_exif_xmp_and_text() {
        let pixels = RgbaImage::from_fn(96, 64, |x, y| {
            image::Rgba([
                (x * 3) as u8,
                (y * 5) as u8,
                (x + y) as u8,
                if x < 32 { 0 } else { 170 },
            ])
        });
        let metadata = Metadata {
            icc: Some(moxcms::ColorProfile::new_srgb().encode().unwrap()),
            exif: Some(vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
                0, 0,
            ]),
            xmp: Some(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>".to_vec()),
            png_text: vec![
                (*b"tEXt", b"Comment\0Keep this text".to_vec()),
                (*b"pHYs", vec![0, 0, 11, 19, 0, 0, 11, 19, 1]),
            ],
            other_metadata: false,
        };
        for strategy in [Strategy::Fast, Strategy::Smart, Strategy::Deep] {
            let (bytes, _) = encode_png(pixels.clone(), &metadata, strategy).unwrap();
            let validated = source_from_candidate(&bytes).unwrap();
            assert_eq!(decode(&validated).unwrap(), pixels);
            assert_eq!(validated.metadata.icc, metadata.icc);
            assert_eq!(validated.metadata.exif, metadata.exif);
            assert_eq!(validated.metadata.xmp, metadata.xmp);
            for chunk in &metadata.png_text {
                assert!(validated.metadata.png_text.contains(chunk));
            }
            assert!(matches!(bytes[25], 2 | 3 | 6));
        }
    }

    #[test]
    fn fast_png_content_paths_keep_exact_pixels() {
        for (gray, opaque) in [(true, true), (true, false), (false, true), (false, false)] {
            let pixels = RgbaImage::from_fn(96, 64, |x, y| {
                let v = (x * 17) as u8;
                image::Rgba([
                    v,
                    if gray { v } else { (y * 33) as u8 },
                    if gray { v } else { (x + y) as u8 },
                    if opaque { 255 } else { (x + y) as u8 },
                ])
            });
            let (encoded, _) =
                encode_png(pixels.clone(), &Metadata::default(), Strategy::Fast).unwrap();
            assert_eq!(
                decode(&source_from_candidate(&encoded).unwrap()).unwrap(),
                pixels
            );
        }
    }
}
