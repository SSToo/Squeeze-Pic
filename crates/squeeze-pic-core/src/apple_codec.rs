//! macOS system codecs for HEIF and AVIF. No external runtime library is required.
use crate::types::*;
use core_foundation::{
    base::{CFRelease, TCFType},
    data::CFData,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use image::RgbaImage;
use std::{ffi::c_void, ptr, slice};
type Ref = *const c_void;
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}
fn owned(value: Ref) -> Result<Owned> {
    if value.is_null() {
        Err(err("codec", "macOS codec could not process this image"))
    } else {
        Ok(Owned(value))
    }
}
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetCount(source: Ref) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageDestinationCreateWithData(data: Ref, kind: Ref, count: usize, options: Ref) -> Ref;
    fn CGImageDestinationAddImage(destination: Ref, image: Ref, properties: Ref);
    fn CGImageDestinationFinalize(destination: Ref) -> bool;
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
    static kCGImagePropertyDepth: Ref;
    static kCGImagePropertyOrientation: Ref;
    static kCGImageDestinationLossyCompressionQuality: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGColorSpaceSRGB: Ref;
    fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
    fn CGContextRelease(context: Ref);
    fn CGDataProviderCreateWithCFData(data: Ref) -> Ref;
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits: usize,
        pixel_bits: usize,
        row: usize,
        space: Ref,
        info: u32,
        provider: Ref,
        decode: Ref,
        interpolate: bool,
        intent: u32,
    ) -> Ref;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: i32, value: *mut c_void) -> bool;
    fn CFDataCreateMutable(allocator: Ref, capacity: isize) -> Ref;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
}
unsafe fn number(properties: Ref, key: Ref, default: u32) -> u32 {
    let n = unsafe { CFDictionaryGetValue(properties, key) };
    if n.is_null() {
        return default;
    }
    let mut value = 0i64;
    if unsafe { CFNumberGetValue(n, 4, (&mut value as *mut i64).cast()) } {
        value.max(0) as u32
    } else {
        default
    }
}
pub fn decode(bytes: &[u8]) -> Result<(RgbaImage, u8)> {
    let data = CFData::from_buffer(bytes);
    let source = owned(unsafe { CGImageSourceCreateWithData(data.as_CFTypeRef(), ptr::null()) })?;
    if unsafe { CGImageSourceGetCount(source.0) } != 1 {
        return Err(err(
            "animation",
            "Animated or multi-image HEIF/AVIF is not supported",
        ));
    }
    let props = owned(unsafe { CGImageSourceCopyPropertiesAtIndex(source.0, 0, ptr::null()) })?;
    let (w, h, depth, orientation) = unsafe {
        (
            number(props.0, kCGImagePropertyPixelWidth, 0),
            number(props.0, kCGImagePropertyPixelHeight, 0),
            number(props.0, kCGImagePropertyDepth, 8),
            number(props.0, kCGImagePropertyOrientation, 1),
        )
    };
    super::codecs::dimensions(w, h)?;
    if depth > 8 {
        return Err(err("bit_depth", "16-bit and HDR images are not supported"));
    }
    let image = owned(unsafe { CGImageSourceCreateImageAtIndex(source.0, 0, ptr::null()) })?;
    let space = owned(unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) })?;
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            w as usize,
            h as usize,
            8,
            w as usize * 4,
            space.0,
            1,
        )
    };
    if context.is_null() {
        return Err(err("decode", "Could not allocate macOS image context"));
    }
    unsafe {
        CGContextDrawImage(
            context,
            Rect {
                origin: Point { x: 0., y: 0. },
                size: Size {
                    width: w as f64,
                    height: h as f64,
                },
            },
            image.0,
        );
        CGContextRelease(context);
    }
    super::codecs::unpremultiply(&mut pixels);
    Ok((
        RgbaImage::from_raw(w, h, pixels).unwrap(),
        orientation.clamp(1, 8) as u8,
    ))
}
pub fn encode_heif(pixels: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let data = CFData::from_buffer(pixels);
    let provider = owned(unsafe { CGDataProviderCreateWithCFData(data.as_CFTypeRef()) })?;
    let space = owned(unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) })?;
    let image = owned(unsafe {
        CGImageCreate(
            pixels.width() as usize,
            pixels.height() as usize,
            8,
            32,
            pixels.width() as usize * 4,
            space.0,
            3,
            provider.0,
            ptr::null(),
            false,
            0,
        )
    })?;
    let output = owned(unsafe { CFDataCreateMutable(ptr::null(), 0) })?;
    let kind = CFString::new("public.heic");
    let destination = owned(unsafe {
        CGImageDestinationCreateWithData(output.0, kind.as_CFTypeRef(), 1, ptr::null())
    })?;
    let key =
        unsafe { CFString::wrap_under_get_rule(kCGImageDestinationLossyCompressionQuality.cast()) };
    let value = CFNumber::from(f64::from(quality) / 100.);
    let properties = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    unsafe {
        CGImageDestinationAddImage(destination.0, image.0, properties.as_CFTypeRef());
    }
    if !unsafe { CGImageDestinationFinalize(destination.0) } {
        return Err(err("encode", "macOS HEIF encoding failed"));
    }
    let len = unsafe { CFDataGetLength(output.0) } as usize;
    if len as u64 > MAX_INPUT {
        return Err(err("limits", "HEIF exceeds the output size limit"));
    }
    Ok(unsafe { slice::from_raw_parts(CFDataGetBytePtr(output.0), len) }.to_vec())
}
