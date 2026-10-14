use crate::types::*;

#[derive(Default, Clone)]
pub(crate) struct Metadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    pub png_text: Vec<([u8; 4], Vec<u8>)>,
    pub other_metadata: bool,
}

pub(crate) fn png_chunks(bytes: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(err("decode", "Invalid PNG signature"));
    }
    let mut chunks = Vec::new();
    let mut offset = 8usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or_else(|| err("decode", "Truncated PNG chunk"))?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .ok_or_else(|| err("limits", "PNG chunk overflow"))?;
        let chunk = bytes
            .get(offset + 4..end - 4)
            .ok_or_else(|| err("decode", "Truncated PNG chunk"))?;
        let crc = bytes
            .get(end - 4..end)
            .ok_or_else(|| err("decode", "Truncated PNG CRC"))?;
        if crc32fast::hash(chunk).to_be_bytes() != crc {
            return Err(err("decode", "PNG chunk checksum mismatch"));
        }
        let tag = header[4..8].try_into().unwrap();
        chunks.push((tag, &bytes[offset + 8..end - 4]));
        offset = end;
        if &tag == b"IEND" {
            if offset != bytes.len() {
                return Err(err("decode", "Unexpected bytes after PNG end"));
            }
            return Ok(chunks);
        }
    }
    Err(err("decode", "PNG end chunk missing"))
}

pub(crate) fn riff_chunks(bytes: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(err("decode", "Invalid WebP container"));
    }
    if u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 != bytes.len() {
        return Err(err("decode", "WebP container size mismatch"));
    }
    let mut chunks = Vec::new();
    let mut offset = 12usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or_else(|| err("decode", "Truncated WebP chunk"))?;
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .ok_or_else(|| err("limits", "WebP chunk overflow"))?;
        let data = bytes
            .get(offset + 8..end)
            .ok_or_else(|| err("decode", "Truncated WebP chunk"))?;
        chunks.push((header[..4].try_into().unwrap(), data));
        offset = end + (length & 1);
        if offset > bytes.len() {
            return Err(err("decode", "Missing WebP padding"));
        }
    }
    Ok(chunks)
}

fn u16_at(bytes: &[u8], i: usize, le: bool) -> Option<u16> {
    let a = bytes.get(i..i.checked_add(2)?)?.try_into().ok()?;
    Some(if le {
        u16::from_le_bytes(a)
    } else {
        u16::from_be_bytes(a)
    })
}
fn u32_at(bytes: &[u8], i: usize, le: bool) -> Option<u32> {
    let a = bytes.get(i..i.checked_add(4)?)?.try_into().ok()?;
    Some(if le {
        u32::from_le_bytes(a)
    } else {
        u32::from_be_bytes(a)
    })
}
fn tiff(bytes: &[u8]) -> Option<(&[u8], bool, usize)> {
    let base = if bytes.starts_with(b"Exif\0\0") { 6 } else { 0 };
    let b = bytes.get(base..)?;
    let le = match b.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if u16_at(b, 2, le)? != 42 {
        return None;
    }
    Some((b, le, base))
}
pub(crate) fn orientation(exif: Option<&[u8]>) -> u8 {
    let Some((b, le, _)) = exif.and_then(tiff) else {
        return 1;
    };
    let Some(offset) = u32_at(b, 4, le).map(|n| n as usize) else {
        return 1;
    };
    let count = u16_at(b, offset, le).unwrap_or(0).min(1024) as usize;
    for i in 0..count {
        let p = offset + 2 + 12 * i;
        if u16_at(b, p, le) == Some(0x112)
            && u16_at(b, p + 2, le) == Some(3)
            && u32_at(b, p + 4, le) == Some(1)
        {
            return u16_at(b, p + 8, le)
                .filter(|v| (1..=8).contains(v))
                .unwrap_or(1) as u8;
        }
    }
    1
}
pub(crate) fn normalize_exif(bytes: &mut [u8], width: u32, height: u32) -> Result<()> {
    let (le, base, first) = {
        let (b, le, base) =
            tiff(bytes).ok_or_else(|| err("metadata", "Cannot safely update malformed EXIF"))?;
        (le, base, u32_at(b, 4, le).unwrap_or(0) as usize)
    };
    let mut offsets = vec![first];
    let mut visited = Vec::new();
    while let Some(offset) = offsets.pop() {
        if offset == 0 || visited.contains(&offset) {
            continue;
        }
        if visited.len() >= 8 {
            return Err(err("metadata", "EXIF nesting exceeds limit"));
        }
        visited.push(offset);
        let start = base + offset;
        let count = u16_at(bytes, start, le)
            .ok_or_else(|| err("metadata", "Truncated EXIF directory"))?
            as usize;
        let end = start
            .checked_add(2 + count * 12)
            .ok_or_else(|| err("metadata", "EXIF directory overflow"))?;
        if count > 1024 || bytes.get(end..end + 4).is_none() {
            return Err(err("metadata", "EXIF directory exceeds limit"));
        }
        for i in 0..count {
            let p = start + 2 + 12 * i;
            let tag = u16_at(bytes, p, le).unwrap();
            let typ = u16_at(bytes, p + 2, le).unwrap();
            let n = u32_at(bytes, p + 4, le).unwrap();
            if tag == 0x8769 && typ == 4 && n == 1 {
                offsets.push(u32_at(bytes, p + 8, le).unwrap() as usize);
            }
            let value = match tag {
                0x112 => Some(1),
                0x100 | 0xa002 => Some(width),
                0x101 | 0xa003 => Some(height),
                _ => None,
            };
            if let Some(value) = value
                && n == 1
            {
                if typ == 3 && value <= u16::MAX.into() {
                    let v = value as u16;
                    bytes[p + 8..p + 10].copy_from_slice(&if le {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    });
                } else if typ == 4 {
                    bytes[p + 8..p + 12].copy_from_slice(&if le {
                        value.to_le_bytes()
                    } else {
                        value.to_be_bytes()
                    });
                }
            }
        }
        // A thumbnail describes the old pixels; remove its directory link.
        bytes[end..end + 4].fill(0);
    }
    Ok(())
}

pub(crate) fn attach_jpeg(bytes: Vec<u8>, metadata: &Metadata) -> Result<Vec<u8>> {
    let mut out = vec![0xff, 0xd8];
    let mut append = |tag: u8, data: &[u8]| -> Result<()> {
        let length: u16 = (data.len() + 2)
            .try_into()
            .map_err(|_| err("metadata", "JPEG metadata segment is too large"))?;
        out.extend([0xff, tag]);
        out.extend(length.to_be_bytes());
        out.extend(data);
        Ok(())
    };
    if let Some(exif) = &metadata.exif {
        let mut data = b"Exif\0\0".to_vec();
        data.extend(exif.strip_prefix(b"Exif\0\0").unwrap_or(exif));
        append(0xe1, &data)?;
    }
    if let Some(xmp) = &metadata.xmp {
        let mut data = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        data.extend(xmp);
        append(0xe1, &data)?;
    }
    if let Some(icc) = &metadata.icc {
        let parts = icc.chunks(65_519).collect::<Vec<_>>();
        let total = u8::try_from(parts.len())
            .map_err(|_| err("metadata", "ICC profile exceeds JPEG segment limit"))?;
        for (i, part) in parts.iter().enumerate() {
            let mut data = b"ICC_PROFILE\0".to_vec();
            data.extend([i as u8 + 1, total]);
            data.extend(*part);
            append(0xe2, &data)?;
        }
    }
    out.extend(&bytes[2..]);
    Ok(out)
}
pub(crate) fn png_extra_chunks(metadata: &Metadata) -> Vec<([u8; 4], Vec<u8>)> {
    let mut chunks = metadata.png_text.clone();
    if let Some(xmp) = &metadata.xmp {
        let mut data = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        data.extend(xmp);
        chunks.push((*b"iTXt", data));
    }
    chunks
}
pub(crate) fn attach_png(bytes: Vec<u8>, metadata: &Metadata) -> Vec<u8> {
    let chunks = png_extra_chunks(metadata);
    if chunks.is_empty() {
        return bytes;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend(&bytes[..33]);
    for (tag, data) in chunks {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(tag);
        out.extend(&data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(&tag);
        crc.update(&data);
        out.extend(crc.finalize().to_be_bytes());
    }
    out.extend(&bytes[33..]);
    out
}
fn riff_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend(tag);
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}
pub(crate) fn attach_webp(
    bytes: Vec<u8>,
    metadata: &Metadata,
    width: u32,
    height: u32,
    alpha: bool,
) -> Result<Vec<u8>> {
    if metadata.icc.is_none() && metadata.exif.is_none() && metadata.xmp.is_none() {
        return Ok(bytes);
    }
    let mut out = b"RIFF\0\0\0\0WEBP".to_vec();
    let flags = if metadata.icc.is_some() { 0x20 } else { 0 }
        | if alpha { 0x10 } else { 0 }
        | if metadata.exif.is_some() { 0x08 } else { 0 }
        | if metadata.xmp.is_some() { 0x04 } else { 0 };
    let mut extended = vec![flags, 0, 0, 0];
    extended.extend(&(width - 1).to_le_bytes()[..3]);
    extended.extend(&(height - 1).to_le_bytes()[..3]);
    riff_chunk(&mut out, b"VP8X", &extended);
    if let Some(icc) = &metadata.icc {
        riff_chunk(&mut out, b"ICCP", icc);
    }
    for (tag, data) in riff_chunks(&bytes)? {
        if matches!(&tag, b"VP8 " | b"VP8L" | b"ALPH") {
            riff_chunk(&mut out, &tag, data);
        }
    }
    if let Some(exif) = &metadata.exif {
        riff_chunk(
            &mut out,
            b"EXIF",
            exif.strip_prefix(b"Exif\0\0").unwrap_or(exif),
        );
    }
    if let Some(xmp) = &metadata.xmp {
        riff_chunk(&mut out, b"XMP ", xmp);
    }
    let length = u32::try_from(out.len() - 8)
        .map_err(|_| err("metadata", "WebP exceeds container limit"))?;
    out[4..8].copy_from_slice(&length.to_le_bytes());
    Ok(out)
}
