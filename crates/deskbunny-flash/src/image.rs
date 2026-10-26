//! Firmware images: Intel HEX parse/emit, raw binaries and face blob patching.

use std::fmt::Write as _;

use crate::{FlashError, Result};

/// Magic that starts the 64-byte face blob inside every firmware image.
pub const FACE_MAGIC: &[u8; 8] = b"DBFACE01";
/// Size of the face blob (docs/ARCHITECTURE.md, "Face blob v1").
pub const FACE_BLOB_SIZE: usize = 64;

/// A contiguous run of bytes at an absolute address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub addr: u32,
    pub data: Vec<u8>,
}

impl Segment {
    fn end(&self) -> u64 {
        self.addr as u64 + self.data.len() as u64
    }
}

/// Start address records carried through a HEX round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAddress {
    /// Record type 03: CS:IP.
    Segment { cs: u16, ip: u16 },
    /// Record type 05: EIP.
    Linear(u32),
}

/// A firmware image as a sorted list of non-overlapping, non-adjacent segments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirmwareImage {
    segments: Vec<Segment>,
    pub start: Option<StartAddress>,
}

impl FirmwareImage {
    /// Build an image from raw bytes loaded at `base` (a `.bin` file).
    pub fn from_bin(base: u32, data: &[u8]) -> Self {
        let mut img = FirmwareImage::default();
        if !data.is_empty() {
            img.segments.push(Segment {
                addr: base,
                data: data.to_vec(),
            });
        }
        img
    }

    /// Build an image from explicit segments (sorted and merged).
    pub fn from_segments(segments: Vec<Segment>) -> Result<Self> {
        let mut img = FirmwareImage {
            segments,
            start: None,
        };
        img.normalize()?;
        Ok(img)
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|s| s.data.is_empty())
    }

    /// Lowest address with data.
    pub fn base(&self) -> u32 {
        self.segments.first().map_or(0, |s| s.addr)
    }

    /// One past the highest address with data.
    pub fn end(&self) -> u32 {
        self.segments.last().map_or(0, |s| s.end() as u32)
    }

    /// Number of data bytes (gaps excluded).
    pub fn data_len(&self) -> usize {
        self.segments.iter().map(|s| s.data.len()).sum()
    }

    /// Flatten `[start, end)` into one buffer, filling gaps with `fill`.
    pub fn flatten(&self, start: u32, end: u32, fill: u8) -> Vec<u8> {
        let mut out = vec![fill; end.saturating_sub(start) as usize];
        for seg in &self.segments {
            let s = seg.addr.max(start);
            let e = (seg.end() as u32).min(end);
            if s >= e {
                continue;
            }
            let src = &seg.data[(s - seg.addr) as usize..(e - seg.addr) as usize];
            out[(s - start) as usize..(e - start) as usize].copy_from_slice(src);
        }
        out
    }

    /// Flatten from the base address rounded down to `page` up to the end rounded up
    /// to `page`, filling gaps with 0xFF (erased flash).
    pub fn paged(&self, page: u32) -> (u32, Vec<u8>) {
        let start = self.base() / page * page;
        let end = self.end().div_ceil(page) * page;
        (start, self.flatten(start, end, 0xFF))
    }

    /// Pages of `page` bytes that contain image data, gaps filled with 0xFF. Pages
    /// with no data at all are skipped (the bootloaders erase per page on write).
    pub fn pages(&self, page: u32) -> Vec<(u32, Vec<u8>)> {
        let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
        for seg in &self.segments {
            let first = seg.addr / page;
            let last = (seg.end() as u32 - 1) / page;
            for p in first..=last {
                let addr = p * page;
                if out.last().is_some_and(|(a, _)| *a == addr) {
                    continue;
                }
                out.push((addr, self.flatten(addr, addr + page, 0xFF)));
            }
        }
        out
    }

    fn normalize(&mut self) -> Result<()> {
        self.segments.retain(|s| !s.data.is_empty());
        self.segments.sort_by_key(|s| s.addr);
        let mut merged: Vec<Segment> = Vec::with_capacity(self.segments.len());
        for seg in self.segments.drain(..) {
            if let Some(last) = merged.last_mut() {
                if (seg.addr as u64) < last.end() {
                    return Err(FlashError::Image(format!(
                        "firmware image has overlapping data at 0x{:08X}",
                        seg.addr
                    )));
                }
                if seg.addr as u64 == last.end() {
                    last.data.extend_from_slice(&seg.data);
                    continue;
                }
            }
            merged.push(seg);
        }
        self.segments = merged;
        Ok(())
    }

    /// Parse Intel HEX text (record types 00-05, checksums enforced).
    pub fn from_hex(text: &str) -> Result<Self> {
        let mut segments: Vec<Segment> = Vec::new();
        let mut upper: u32 = 0; // from type 02 (segment << 4) or 04 (linear << 16)
        let mut start = None;
        let mut saw_eof = false;

        for (idx, raw) in text.lines().enumerate() {
            let lineno = idx + 1;
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if saw_eof {
                return Err(hex_err(lineno, "data after end-of-file record"));
            }
            let body = line
                .strip_prefix(':')
                .ok_or_else(|| hex_err(lineno, "line does not start with ':'"))?;
            if body.len() % 2 != 0 || body.len() < 10 {
                return Err(hex_err(lineno, "malformed record length"));
            }
            let bytes = decode_hex(body).ok_or_else(|| hex_err(lineno, "invalid hex digit"))?;
            let count = bytes[0] as usize;
            if bytes.len() != count + 5 {
                return Err(hex_err(lineno, "byte count does not match record length"));
            }
            let sum = bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b));
            if sum != 0 {
                return Err(hex_err(lineno, "checksum mismatch"));
            }
            let offset = u16::from_be_bytes([bytes[1], bytes[2]]) as u32;
            let rtype = bytes[3];
            let data = &bytes[4..4 + count];
            match rtype {
                0x00 => {
                    // Addresses wrap within the 64 KiB window for type 04 (and type 02
                    // per spec); handle a record crossing the boundary byte by byte.
                    let mut push = |addr: u32, chunk: &[u8]| match segments.last_mut() {
                        Some(last) if last.end() == addr as u64 => {
                            last.data.extend_from_slice(chunk)
                        }
                        _ => segments.push(Segment {
                            addr,
                            data: chunk.to_vec(),
                        }),
                    };
                    let first = (0x1_0000 - offset as usize).min(count);
                    push(upper.wrapping_add(offset), &data[..first]);
                    if first < count {
                        push(upper, &data[first..]);
                    }
                }
                0x01 => saw_eof = true,
                0x02 => {
                    if count != 2 {
                        return Err(hex_err(
                            lineno,
                            "extended segment address must have 2 bytes",
                        ));
                    }
                    upper = (u16::from_be_bytes([data[0], data[1]]) as u32) << 4;
                }
                0x03 => {
                    if count != 4 {
                        return Err(hex_err(lineno, "start segment address must have 4 bytes"));
                    }
                    start = Some(StartAddress::Segment {
                        cs: u16::from_be_bytes([data[0], data[1]]),
                        ip: u16::from_be_bytes([data[2], data[3]]),
                    });
                }
                0x04 => {
                    if count != 2 {
                        return Err(hex_err(lineno, "extended linear address must have 2 bytes"));
                    }
                    upper = (u16::from_be_bytes([data[0], data[1]]) as u32) << 16;
                }
                0x05 => {
                    if count != 4 {
                        return Err(hex_err(lineno, "start linear address must have 4 bytes"));
                    }
                    start = Some(StartAddress::Linear(u32::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                    ])));
                }
                t => return Err(hex_err(lineno, &format!("unknown record type {t:02X}"))),
            }
        }
        if !saw_eof {
            return Err(FlashError::Image(
                "Intel HEX file is truncated (no end-of-file record)".into(),
            ));
        }
        let mut img = FirmwareImage { segments, start };
        img.normalize()?;
        Ok(img)
    }

    /// Emit Intel HEX with 16-byte data records and type 04 records as needed.
    pub fn to_hex(&self) -> String {
        let mut out = String::new();
        let mut upper: u32 = 0;
        for seg in &self.segments {
            let mut addr = seg.addr;
            let mut rest = &seg.data[..];
            while !rest.is_empty() {
                if addr >> 16 != upper {
                    upper = addr >> 16;
                    push_record(&mut out, 0, 0x04, &(upper as u16).to_be_bytes());
                }
                // Never let a record cross a 64 KiB boundary.
                let room = 0x1_0000 - (addr & 0xFFFF) as usize;
                let n = rest.len().min(16).min(room);
                push_record(&mut out, (addr & 0xFFFF) as u16, 0x00, &rest[..n]);
                addr = addr.wrapping_add(n as u32);
                rest = &rest[n..];
            }
        }
        match self.start {
            Some(StartAddress::Segment { cs, ip }) => {
                let mut d = cs.to_be_bytes().to_vec();
                d.extend_from_slice(&ip.to_be_bytes());
                push_record(&mut out, 0, 0x03, &d);
            }
            Some(StartAddress::Linear(eip)) => push_record(&mut out, 0, 0x05, &eip.to_be_bytes()),
            None => {}
        }
        push_record(&mut out, 0, 0x01, &[]);
        out
    }

    /// Every absolute address where `needle` occurs (searched within segments).
    pub fn find_all(&self, needle: &[u8]) -> Vec<u32> {
        let mut hits = Vec::new();
        for seg in &self.segments {
            if seg.data.len() < needle.len() {
                continue;
            }
            for (i, w) in seg.data.windows(needle.len()).enumerate() {
                if w == needle {
                    hits.push(seg.addr + i as u32);
                }
            }
        }
        hits
    }

    /// Overwrite bytes at `addr`; the whole range must already hold data.
    pub fn write_at(&mut self, addr: u32, bytes: &[u8]) -> Result<()> {
        let seg = self
            .segments
            .iter_mut()
            .find(|s| s.addr <= addr && addr as u64 + bytes.len() as u64 <= s.end())
            .ok_or_else(|| {
                FlashError::Image(format!(
                    "cannot write {} bytes at 0x{addr:08X}: outside image data",
                    bytes.len()
                ))
            })?;
        let off = (addr - seg.addr) as usize;
        seg.data[off..off + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Read bytes at `addr` if fully covered by one segment.
    pub fn read_at(&self, addr: u32, len: usize) -> Option<&[u8]> {
        self.segments
            .iter()
            .find(|s| s.addr <= addr && addr as u64 + len as u64 <= s.end())
            .map(|s| {
                let off = (addr - s.addr) as usize;
                &s.data[off..off + len]
            })
    }
}

/// Locate the face blob (exactly one `DBFACE01`) and overwrite its 64 bytes.
/// Returns the flash address of the blob.
pub fn patch_face(image: &mut FirmwareImage, blob: &[u8; FACE_BLOB_SIZE]) -> Result<u32> {
    if &blob[..8] != FACE_MAGIC {
        return Err(FlashError::Image(
            "face blob does not start with DBFACE01".into(),
        ));
    }
    let hits = image.find_all(FACE_MAGIC);
    let addr = match hits.as_slice() {
        [one] => *one,
        [] => {
            return Err(FlashError::Image(
                "firmware image has no face slot (DBFACE01 not found); the bundled firmware is damaged or not a DeskBunny build".into(),
            ))
        }
        many => {
            return Err(FlashError::Image(format!(
                "firmware image has {} DBFACE01 markers; expected exactly one",
                many.len()
            )))
        }
    };
    image.write_at(addr, blob).map_err(|_| {
        FlashError::Image(format!(
            "face slot at 0x{addr:08X} is truncated: fewer than 64 bytes of image data follow the marker"
        ))
    })?;
    Ok(addr)
}

fn push_record(out: &mut String, addr: u16, rtype: u8, data: &[u8]) {
    let mut sum = data.len() as u8;
    sum = sum
        .wrapping_add((addr >> 8) as u8)
        .wrapping_add(addr as u8)
        .wrapping_add(rtype);
    let _ = write!(out, ":{:02X}{:04X}{:02X}", data.len(), addr, rtype);
    for b in data {
        sum = sum.wrapping_add(*b);
        let _ = write!(out, "{b:02X}");
    }
    let _ = writeln!(out, "{:02X}", sum.wrapping_neg());
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    (0..b.len())
        .step_by(2)
        .map(|i| {
            let hi = (b[i] as char).to_digit(16)?;
            let lo = (b[i + 1] as char).to_digit(16)?;
            Some((hi * 16 + lo) as u8)
        })
        .collect()
}

fn hex_err(line: usize, msg: &str) -> FlashError {
    FlashError::Image(format!("Intel HEX line {line}: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(fill: u8) -> [u8; 64] {
        let mut b = [fill; 64];
        b[..8].copy_from_slice(FACE_MAGIC);
        b
    }

    #[test]
    fn parses_known_record() {
        // Classic example record from the Intel HEX spec.
        let hex = ":10010000214601360121470136007EFE09D2190140\n:00000001FF\n";
        let img = FirmwareImage::from_hex(hex).unwrap();
        assert_eq!(img.base(), 0x100);
        assert_eq!(img.data_len(), 16);
        assert_eq!(img.read_at(0x100, 2).unwrap(), &[0x21, 0x46]);
        assert_eq!(img.to_hex(), hex);
    }

    #[test]
    fn rejects_bad_checksum_and_missing_eof() {
        let bad = ":10010000214601360121470136007EFE09D2190141\n:00000001FF\n";
        let e = FirmwareImage::from_hex(bad).unwrap_err().to_string();
        assert!(e.contains("line 1") && e.contains("checksum"), "{e}");
        let trunc = ":10010000214601360121470136007EFE09D2190140\n";
        assert!(FirmwareImage::from_hex(trunc).is_err());
        assert!(FirmwareImage::from_hex(":0000000AFF\n:00000001FF\n").is_err());
    }

    #[test]
    fn extended_segment_and_linear_addressing() {
        let mut hex = String::new();
        push_record(&mut hex, 0, 0x02, &[0x10, 0x00]); // base 0x10000
        push_record(&mut hex, 0x0010, 0x00, &[1, 2, 3, 4]);
        push_record(&mut hex, 0, 0x04, &[0x00, 0x02]); // base 0x20000
        push_record(&mut hex, 0xFFFE, 0x00, &[5, 6]);
        push_record(&mut hex, 0, 0x04, &[0x00, 0x03]);
        push_record(&mut hex, 0x0000, 0x00, &[7, 8]);
        push_record(&mut hex, 0, 0x05, &[0x00, 0x00, 0x40, 0x00]);
        push_record(&mut hex, 0, 0x01, &[]);
        let img = FirmwareImage::from_hex(&hex).unwrap();
        assert_eq!(img.read_at(0x10010, 4).unwrap(), &[1, 2, 3, 4]);
        // 0x2FFFE..0x30002 is contiguous and merged into one segment.
        assert_eq!(img.read_at(0x2FFFE, 4).unwrap(), &[5, 6, 7, 8]);
        assert_eq!(img.segments().len(), 2);
        assert_eq!(img.start, Some(StartAddress::Linear(0x4000)));
        let again = FirmwareImage::from_hex(&img.to_hex()).unwrap();
        assert_eq!(again, img);
    }

    #[test]
    fn emitted_records_do_not_cross_64k() {
        let img = FirmwareImage::from_bin(0xFFF8, &[0xAB; 32]);
        let hex = img.to_hex();
        assert!(hex.contains(":020000040001F9"), "{hex}");
        assert_eq!(FirmwareImage::from_hex(&hex).unwrap(), img);
    }

    #[test]
    fn overlapping_data_is_rejected() {
        let mut hex = String::new();
        push_record(&mut hex, 0x0000, 0x00, &[1, 2, 3, 4]);
        push_record(&mut hex, 0x0002, 0x00, &[9, 9]);
        push_record(&mut hex, 0, 0x01, &[]);
        assert!(FirmwareImage::from_hex(&hex).is_err());
    }

    #[test]
    fn patch_blob_straddling_records() {
        // Program bytes 0..300, blob placed at offset 0x5A (not 16-aligned), so its
        // 64 bytes span five 16-byte HEX records.
        let mut data: Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();
        data[0x5A..0x5A + 64].copy_from_slice(&blob(0));
        let hex = FirmwareImage::from_bin(0, &data).to_hex();
        let mut img = FirmwareImage::from_hex(&hex).unwrap();
        let addr = patch_face(&mut img, &blob(0x42)).unwrap();
        assert_eq!(addr, 0x5A);
        let out = FirmwareImage::from_hex(&img.to_hex()).unwrap();
        let flat = out.flatten(0, 300, 0xFF);
        assert_eq!(&flat[0x5A..0x5A + 8], FACE_MAGIC);
        assert!(flat[0x5A + 8..0x5A + 64].iter().all(|b| *b == 0x42));
        assert_eq!(&flat[..0x5A], &data[..0x5A]);
        assert_eq!(&flat[0x5A + 64..], &data[0x5A + 64..]);
    }

    #[test]
    fn patch_requires_exactly_one_marker() {
        let mut none = FirmwareImage::from_bin(0, &[0u8; 128]);
        assert!(patch_face(&mut none, &blob(1)).is_err());

        let mut data = vec![0u8; 256];
        data[0..8].copy_from_slice(FACE_MAGIC);
        data[100..108].copy_from_slice(FACE_MAGIC);
        let mut two = FirmwareImage::from_bin(0, &data);
        let e = patch_face(&mut two, &blob(1)).unwrap_err().to_string();
        assert!(e.contains("2 DBFACE01"), "{e}");

        let mut short = vec![0u8; 40];
        short[0..8].copy_from_slice(FACE_MAGIC);
        let mut trunc = FirmwareImage::from_bin(0x4000, &short);
        assert!(patch_face(&mut trunc, &blob(1)).is_err());
    }

    #[test]
    fn paged_fills_gaps_with_ff() {
        let img = FirmwareImage::from_segments(vec![
            Segment {
                addr: 2,
                data: vec![1, 2],
            },
            Segment {
                addr: 6,
                data: vec![3],
            },
        ])
        .unwrap();
        let (start, buf) = img.paged(4);
        assert_eq!(start, 0);
        assert_eq!(buf, vec![0xFF, 0xFF, 1, 2, 0xFF, 0xFF, 3, 0xFF]);
        let far = FirmwareImage::from_segments(vec![
            Segment {
                addr: 2,
                data: vec![1, 2, 3],
            },
            Segment {
                addr: 6,
                data: vec![4],
            },
            Segment {
                addr: 100,
                data: vec![5],
            },
        ])
        .unwrap();
        let pages = far.pages(4);
        assert_eq!(
            pages,
            vec![
                (0, vec![0xFF, 0xFF, 1, 2]),
                (4, vec![3, 0xFF, 4, 0xFF]),
                (100, vec![5, 0xFF, 0xFF, 0xFF]),
            ]
        );
    }
}
