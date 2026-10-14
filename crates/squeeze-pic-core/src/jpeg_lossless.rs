// Adapted from Squeeze PDF (Apache-2.0); see THIRD-PARTY.md.
//! jpegtran-style entropy recoding. No inverse DCT, resampling or quantization.
//! Native structs and callbacks stay boxed for stable addresses; C-unwind is
//! explicitly supported by mozjpeg-sys. Both native pools are destroyed on all
//! paths, including errors; the bounded destination is owned entirely by Rust.
use mozjpeg_sys as ffi;
use std::{ffi::c_int, mem, ptr};

type JpegResult<T> = std::result::Result<T, String>;
#[repr(C)]
struct Destination {
    manager: ffi::jpeg_destination_mgr,
    buffer: [u8; 16_384],
    output: Vec<u8>,
    limit: usize,
}
impl Destination {
    fn append(&mut self, count: usize) {
        if self.output.len().saturating_add(count) > self.limit {
            std::panic::resume_unwind(Box::new("JPEG output exceeds limit"));
        }
        self.output.extend_from_slice(&self.buffer[..count]);
    }
}
unsafe extern "C-unwind" fn init(c: &mut ffi::jpeg_compress_struct) {
    // SAFETY: dest always points to the first repr(C) field of our boxed state.
    let d = unsafe { &mut *c.dest.cast::<Destination>() };
    d.output.clear();
    d.manager.next_output_byte = d.buffer.as_mut_ptr();
    d.manager.free_in_buffer = d.buffer.len();
}
unsafe extern "C-unwind" fn empty(c: &mut ffi::jpeg_compress_struct) -> c_int {
    let d = unsafe { &mut *c.dest.cast::<Destination>() };
    d.append(d.buffer.len());
    d.manager.next_output_byte = d.buffer.as_mut_ptr();
    d.manager.free_in_buffer = d.buffer.len();
    1
}
unsafe extern "C-unwind" fn term(c: &mut ffi::jpeg_compress_struct) {
    let d = unsafe { &mut *c.dest.cast::<Destination>() };
    d.append(d.buffer.len() - d.manager.free_in_buffer);
}
unsafe extern "C-unwind" fn fatal(_: &mut ffi::jpeg_common_struct) {
    std::panic::resume_unwind(Box::new("Invalid JPEG coefficient stream"));
}
unsafe extern "C-unwind" fn message(_: &mut ffi::jpeg_common_struct, level: c_int) {
    // Truncated streams must not be silently repaired and called lossless.
    if level < 0 {
        std::panic::resume_unwind(Box::new("JPEG coefficient warning"));
    }
}
struct Native {
    decoder: Box<ffi::jpeg_decompress_struct>,
    encoder: Box<ffi::jpeg_compress_struct>,
    decode_error: Box<ffi::jpeg_error_mgr>,
    encode_error: Box<ffi::jpeg_error_mgr>,
    destination: Box<Destination>,
}
impl Native {
    fn new(limit: usize) -> Self {
        // SAFETY: libjpeg requires zero-initialized structs before creation.
        unsafe {
            let mut native = Self {
                decoder: Box::new(mem::zeroed()),
                encoder: Box::new(mem::zeroed()),
                decode_error: Box::new(mem::zeroed()),
                encode_error: Box::new(mem::zeroed()),
                destination: Box::new(Destination {
                    manager: mem::zeroed(),
                    buffer: [0; 16_384],
                    output: Vec::new(),
                    limit,
                }),
            };
            ffi::jpeg_std_error(&mut native.decode_error);
            ffi::jpeg_std_error(&mut native.encode_error);
            for err in [&mut native.decode_error, &mut native.encode_error] {
                err.error_exit = Some(fatal);
                err.emit_message = Some(message);
            }
            native.decoder.common.err = &mut *native.decode_error;
            native.encoder.common.err = &mut *native.encode_error;
            ffi::jpeg_create_decompress(&mut *native.decoder);
            ffi::jpeg_create_compress(&mut *native.encoder);
            native.destination.manager.init_destination = Some(init);
            native.destination.manager.empty_output_buffer = Some(empty);
            native.destination.manager.term_destination = Some(term);
            native.encoder.dest = &mut native.destination.manager;
            native
        }
    }
    unsafe fn coefficients(
        &mut self,
        bytes: &[u8],
        dimensions: [u32; 2],
        max_pixels: u64,
    ) -> JpegResult<*mut *mut ffi::jvirt_barray_control> {
        unsafe {
            ffi::jpeg_mem_src(
                &mut self.decoder,
                bytes.as_ptr(),
                bytes.len().try_into().map_err(|_| "JPEG input too large")?,
            );
            for marker in (0xe0..=0xef).chain([0xfe]) {
                ffi::jpeg_save_markers(&mut self.decoder, marker, 0xffff);
            }
            if ffi::jpeg_read_header(&mut self.decoder, 1) != 1 {
                return Err("JPEG header invalid".into());
            }
            let d = &self.decoder;
            if [d.image_width, d.image_height] != dimensions
                || !matches!(d.num_components, 1 | 3 | 4)
                || !matches!(
                    d.jpeg_color_space,
                    ffi::JCS_GRAYSCALE
                        | ffi::JCS_RGB
                        | ffi::JCS_YCbCr
                        | ffi::JCS_CMYK
                        | ffi::JCS_YCCK
                )
                || dimensions[0] as u64 * dimensions[1] as u64 > max_pixels
            {
                return Err("Unsupported JPEG dimensions or color".into());
            }
            // Bound coefficient buffers separately from decoded 8-bit pixels.
            let padded =
                (dimensions[0] as u64).div_ceil(8) * 8 * ((dimensions[1] as u64).div_ceil(8) * 8);
            if padded * d.num_components as u64 * 2 > self.destination.limit as u64 {
                return Err("JPEG coefficients exceed limit".into());
            }
            let coefficients = ffi::jpeg_read_coefficients(&mut self.decoder);
            if coefficients.is_null() {
                return Err("Missing JPEG coefficients".into());
            }
            Ok(coefficients)
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        // Creation can fail partway; non-null mem identifies allocated pools.
        unsafe {
            if !self.encoder.common.mem.is_null() {
                ffi::jpeg_destroy_compress(&mut self.encoder);
            }
            if !self.decoder.common.mem.is_null() {
                ffi::jpeg_destroy_decompress(&mut self.decoder);
            }
        }
    }
}
fn scan_mode(bytes: &[u8]) -> Option<bool> {
    frame(bytes).map(|(progressive, _)| progressive)
}
pub(crate) fn components(bytes: &[u8]) -> Option<u8> {
    frame(bytes).map(|(_, components)| components)
}
fn frame(bytes: &[u8]) -> Option<(bool, u8)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut i = 2;
    while i < bytes.len() {
        if bytes[i] != 0xff {
            return None;
        }
        while bytes.get(i) == Some(&0xff) {
            i += 1;
        }
        let marker = *bytes.get(i)?;
        i += 1;
        if matches!(marker, 0xc0..=0xc2) {
            let length = u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]) as usize;
            let components = *bytes.get(i + 7)?;
            if length != 8 + 3 * usize::from(components)
                || bytes.get(i..i.checked_add(length)?).is_none()
            {
                return None;
            }
            return Some((marker == 0xc2, components));
        }
        if matches!(marker, 0xda | 0xd9 | 0x00 | 0xd8) {
            return None;
        }
        let length = u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]) as usize;
        if length < 2 {
            return None;
        }
        i = i.checked_add(length)?;
    }
    None
}

pub(crate) fn optimize(
    bytes: &[u8],
    dimensions: [u32; 2],
    max_pixels: u64,
    limit: usize,
    keep_scan_mode: bool,
) -> JpegResult<Vec<(&'static str, Vec<u8>)>> {
    if bytes.len() > limit || !bytes.starts_with(&[0xff, 0xd8]) || dimensions.contains(&0) {
        return Err("Invalid JPEG input".into());
    }
    std::panic::catch_unwind(|| {
        let mut native = Native::new(limit);
        // SAFETY: boxes keep native addresses alive through both encodes; the
        // shared virtual coefficient arrays belong to the still-live decoder.
        unsafe {
            let source_progressive = if keep_scan_mode {
                scan_mode(bytes).ok_or("Unsupported JPEG scan mode")?
            } else {
                false
            };
            let coefficients = native.coefficients(bytes, dimensions, max_pixels)?;
            let mut outputs = Vec::new();
            for progressive in [false, true] {
                if keep_scan_mode && source_progressive != progressive {
                    continue;
                }
                let c = &mut native.encoder;
                ffi::jpeg_copy_critical_parameters(&native.decoder, c);
                c.optimize_coding = 1;
                c.scan_info = ptr::null();
                c.num_scans = 0;
                c.write_JFIF_header = 0;
                c.write_Adobe_marker = 0;
                ffi::jpeg_c_set_bool_param(c, ffi::JBOOLEAN_TRELLIS_QUANT, 0);
                ffi::jpeg_c_set_bool_param(c, ffi::JBOOLEAN_TRELLIS_QUANT_DC, 0);
                ffi::jpeg_c_set_bool_param(c, ffi::JBOOLEAN_TRELLIS_Q_OPT, 0);
                ffi::jpeg_c_set_bool_param(c, ffi::JBOOLEAN_OPTIMIZE_SCANS, progressive.into());
                if progressive {
                    ffi::jpeg_simple_progression(c);
                }
                ffi::jpeg_write_coefficients(c, coefficients);
                let mut marker = native.decoder.marker_list;
                while let Some(m) = marker.as_ref() {
                    ffi::jpeg_write_marker(c, m.marker as c_int, m.data, m.data_length);
                    marker = m.next;
                }
                ffi::jpeg_finish_compress(c);
                outputs.push((
                    if progressive {
                        "jpeg-lossless/progressive"
                    } else {
                        "jpeg-lossless/sequential"
                    },
                    mem::take(&mut native.destination.output),
                ));
                ffi::jpeg_abort_compress(c);
            }
            ffi::jpeg_finish_decompress(&mut native.decoder);
            Ok(outputs)
        }
    })
    .map_err(|_| "JPEG coefficient optimization failed".to_string())?
}

pub(crate) fn validate(
    bytes: &[u8],
    dimensions: [u32; 2],
    max_pixels: u64,
    limit: usize,
) -> JpegResult<()> {
    std::panic::catch_unwind(|| {
        let mut native = Native::new(limit);
        // SAFETY: the decoder owns its coefficient arrays until Native drops.
        unsafe {
            native.coefficients(bytes, dimensions, max_pixels)?;
            ffi::jpeg_finish_decompress(&mut native.decoder);
        }
        Ok(())
    })
    .map_err(|_| "JPEG bitstream validation failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    type Fingerprint = (Vec<i16>, Vec<u16>, Vec<i32>, Vec<(u8, Vec<u8>)>);
    fn fingerprint(bytes: &[u8], dims: [u32; 2]) -> Fingerprint {
        let mut n = Native::new(16 * 1024 * 1024);
        unsafe {
            let arrays = n.coefficients(bytes, dims, 1_000_000).unwrap();
            let mut quant = Vec::new();
            let mut sampling = Vec::new();
            let mut coefs = Vec::new();
            let mut markers = Vec::new();
            for &q in &n.decoder.quant_tbl_ptrs {
                if let Some(q) = q.as_ref() {
                    quant.extend(q.quantval);
                }
            }
            let access = (*n.decoder.common.mem).access_virt_barray.unwrap();
            for i in 0..n.decoder.num_components as usize {
                let comp = &*n.decoder.comp_info.add(i);
                sampling.extend([
                    comp.component_id,
                    comp.h_samp_factor,
                    comp.v_samp_factor,
                    comp.quant_tbl_no,
                ]);
                for y in 0..comp.height_in_blocks {
                    let row = access(&mut n.decoder.common, *arrays.add(i), y, 1, 0);
                    for x in 0..comp.width_in_blocks as usize {
                        coefs.extend(*(*row).add(x));
                    }
                }
            }
            let mut marker = n.decoder.marker_list;
            while let Some(m) = marker.as_ref() {
                markers.push((
                    m.marker,
                    std::slice::from_raw_parts(m.data, m.data_length as usize).to_vec(),
                ));
                marker = m.next;
            }
            (coefs, quant, sampling, markers)
        }
    }
    #[test]
    fn entropy_recode_keeps_coefficients_quantization_sampling_markers_and_pixels() {
        for gray in [false, true] {
            for quality in [60, 95] {
                let pixels: Vec<_> = (0..47 * 33 * if gray { 1 } else { 3 })
                    .map(|i| ((i * 37 + i / 47 * 19) % 256) as u8)
                    .collect();
                let mut original = crate::jpeg::encode(&pixels, 47, 33, gray, quality).unwrap();
                // ICC-like APP2, EXIF-like APP1 and COM payloads including zeros.
                for (tag, data) in [
                    (0xe2, &b"ICC_PROFILE\0\x01\x01exact profile"[..]),
                    (0xe1, &b"Exif\0\0exact metadata"[..]),
                    (0xfe, &b"comment\0"[..]),
                ] {
                    let mut segment = vec![0xff, tag];
                    segment.extend(((data.len() + 2) as u16).to_be_bytes());
                    segment.extend(data);
                    original.splice(2..2, segment);
                }
                let before = fingerprint(&original, [47, 33]);
                for (_, out) in
                    optimize(&original, [47, 33], 1_000_000, 16 * 1024 * 1024, false).unwrap()
                {
                    assert_eq!(fingerprint(&out, [47, 33]), before);
                    assert_eq!(
                        image::load_from_memory(&out).unwrap().into_bytes(),
                        image::load_from_memory(&original).unwrap().into_bytes()
                    );
                }
            }
        }
    }
    #[test]
    fn scan_preserving_candidates_retain_progressive_or_sequential_source_mode() {
        let progressive = crate::jpeg::encode(&vec![140; 64 * 64 * 3], 64, 64, false, 95).unwrap();
        let sequential = optimize(&progressive, [64, 64], 1_000_000, 1_000_000, false)
            .unwrap()
            .into_iter()
            .find(|(name, _)| *name == "jpeg-lossless/sequential")
            .unwrap()
            .1;
        for source in [&progressive, &sequential] {
            let candidates = optimize(source, [64, 64], 1_000_000, 1_000_000, true).unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(scan_mode(source), scan_mode(&candidates[0].1));
            assert_eq!(
                fingerprint(source, [64, 64]),
                fingerprint(&candidates[0].1, [64, 64])
            );
        }
        assert_eq!(scan_mode(&[255, 216, 255, 224, 0, 255]), None);
    }
    #[test]
    fn rejects_truncation_mismatched_dimensions_and_memory_budget() {
        let jpg = crate::jpeg::encode(&vec![128; 64 * 64 * 3], 64, 64, false, 85).unwrap();
        for (bytes, dims, limit) in [
            (&jpg[..jpg.len() / 2], [64, 64], 1_000_000),
            (&jpg, [63, 64], 1_000_000),
            (&jpg, [64, 64], 4096),
        ] {
            assert!(optimize(bytes, dims, 1_000_000, limit, false).is_err());
        }
    }
    #[test]
    fn cmyk_ycck_keep_coefficients_sampling_scan_mode_and_adobe_markers() {
        for output in [mozjpeg::ColorSpace::JCS_CMYK, mozjpeg::ColorSpace::JCS_YCCK] {
            let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_CMYK);
            c.set_fastest_defaults();
            c.set_color_space(output);
            c.set_size(47, 33);
            c.set_quality(85.);
            let mut c = c.start_compress(Vec::new()).unwrap();
            let raw: Vec<_> = (0..47 * 33 * 4)
                .map(|i| ((i * 37 + i / 47 * 19) % 256) as u8)
                .collect();
            c.write_scanlines(&raw).unwrap();
            let original = c.finish().unwrap();
            let expected = fingerprint(&original, [47, 33]);
            assert!(
                expected
                    .3
                    .iter()
                    .any(|(m, b)| *m == 0xee && b.starts_with(b"Adobe"))
            );
            for (_, out) in
                optimize(&original, [47, 33], 1_000_000, 16 * 1024 * 1024, true).unwrap()
            {
                assert_eq!(fingerprint(&out, [47, 33]), expected);
                assert_eq!(scan_mode(&out), scan_mode(&original));
                assert_eq!(
                    image::load_from_memory(&out).unwrap().into_bytes(),
                    image::load_from_memory(&original).unwrap().into_bytes()
                );
            }
        }
    }
}
