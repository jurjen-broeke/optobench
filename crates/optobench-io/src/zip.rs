//! Reading ZIP archives: enough for spreadsheet files (`.xlsx`), which are ZIP archives of
//! XML parts.
//!
//! Written from the specifications: the archive layout from PKWARE's APPNOTE.TXT (local
//! headers, central directory, end of central directory), Deflate from RFC 1951 and
//! CRC-32 from ISO 3309 / ITU-T V.42 (the polynomial 0xEDB88320 in reflected form), as
//! APPNOTE requires. Stored and deflated entries only; no ZIP64, no encryption.

/// Largest entry this reader unpacks (bytes), against archives that expand without
/// bound.
const MAX_ENTRY: usize = 256 << 20;

/// Why an archive could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipError(pub String);

impl core::fmt::Display for ZipError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T, ZipError> {
    Err(ZipError(message.into()))
}

/// CRC-32 of `data` (ISO 3309; reflected polynomial 0xEDB88320, initial and final
/// value 0xFFFFFFFF).
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, entry) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// Bits of a Deflate stream, least significant bit of each byte first (RFC 1951, 3.1.1).
struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, ZipError> {
        let byte = *self
            .data
            .get(self.position / 8)
            .ok_or_else(|| ZipError("compressed data ends early".into()))?;
        let bit = (byte >> (self.position % 8)) & 1;
        self.position += 1;
        Ok(u32::from(bit))
    }

    /// `n` bits as a number, first bit least significant.
    fn bits(&mut self, n: u32) -> Result<u32, ZipError> {
        let mut value = 0;
        for i in 0..n {
            value |= self.bit()? << i;
        }
        Ok(value)
    }

    fn align(&mut self) {
        self.position = self.position.div_ceil(8) * 8;
    }
}

/// A canonical Huffman code (RFC 1951, 3.2.2): how many codes of each length, and the
/// symbols in code order.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, ZipError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[usize::from(l)] += 1;
        }
        counts[0] = 0;
        // No length may have more codes than it can hold.
        let mut left: i32 = 1;
        for &count in &counts[1..] {
            left = left * 2 - i32::from(count);
            if left < 0 {
                return fail("invalid Huffman code lengths");
            }
        }
        let mut offsets = [0u16; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(offsets[usize::from(l)])] = symbol as u16;
                offsets[usize::from(l)] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    /// The next symbol: codes are read most significant bit first (RFC 1951, 3.1.1).
    fn decode(&self, bits: &mut Bits) -> Result<u16, ZipError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.bit()? as i32;
            let count = i32::from(self.counts[len]);
            if code - first < count {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        fail("invalid Huffman code in compressed data")
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order code length code lengths are sent in (RFC 1951, 3.2.7).
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn fixed_codes() -> Result<(Huffman, Huffman), ZipError> {
    // RFC 1951, 3.2.6.
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_codes(bits: &mut Bits) -> Result<(Huffman, Huffman), ZipError> {
    // RFC 1951, 3.2.7.
    let literals = bits.bits(5)? as usize + 257;
    let distances = bits.bits(5)? as usize + 1;
    let code_lengths = bits.bits(4)? as usize + 4;
    if literals > 286 || distances > 30 {
        return fail("invalid dynamic Huffman header");
    }
    let mut lengths = [0u8; 19];
    for &i in &CODE_LENGTH_ORDER[..code_lengths] {
        lengths[i] = bits.bits(3)? as u8;
    }
    let code = Huffman::new(&lengths)?;
    let mut all = Vec::with_capacity(literals + distances);
    while all.len() < literals + distances {
        let symbol = code.decode(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => {
                let previous = *all
                    .last()
                    .ok_or_else(|| ZipError("repeat with no previous length".into()))?;
                (previous, 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if all.len() + repeat > literals + distances {
            return fail("code lengths run past their count");
        }
        all.extend(core::iter::repeat_n(value, repeat));
    }
    if all[256] == 0 {
        return fail("no end-of-block code");
    }
    Ok((
        Huffman::new(&all[..literals])?,
        Huffman::new(&all[literals..])?,
    ))
}

/// Decompresses a raw Deflate stream (RFC 1951).
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, ZipError> {
    let mut bits = Bits { data, position: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bit()? == 1;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let start = bits.position / 8;
                let header = data
                    .get(start..start + 4)
                    .ok_or_else(|| ZipError("stored block header ends early".into()))?;
                let len = u16::from_le_bytes([header[0], header[1]]);
                let nlen = u16::from_le_bytes([header[2], header[3]]);
                if len != !nlen {
                    return fail("stored block length check failed");
                }
                let block = data
                    .get(start + 4..start + 4 + usize::from(len))
                    .ok_or_else(|| ZipError("stored block ends early".into()))?;
                out.extend_from_slice(block);
                bits.position = (start + 4 + usize::from(len)) * 8;
            }
            kind @ (1 | 2) => {
                let (literal, distance) = if kind == 1 {
                    fixed_codes()?
                } else {
                    dynamic_codes(&mut bits)?
                };
                loop {
                    let symbol = literal.decode(&mut bits)?;
                    match symbol {
                        0..=255 => out.push(symbol as u8),
                        256 => break,
                        _ => {
                            let i = usize::from(symbol - 257);
                            if i >= 29 {
                                return fail("invalid length code");
                            }
                            let length = usize::from(LENGTH_BASE[i])
                                + bits.bits(u32::from(LENGTH_EXTRA[i]))? as usize;
                            let d = usize::from(distance.decode(&mut bits)?);
                            if d >= 30 {
                                return fail("invalid distance code");
                            }
                            let back = usize::from(DISTANCE_BASE[d])
                                + bits.bits(u32::from(DISTANCE_EXTRA[d]))? as usize;
                            if back > out.len() {
                                return fail("distance reaches before the start");
                            }
                            let from = out.len() - back;
                            for k in 0..length {
                                out.push(out[from + k]);
                            }
                        }
                    }
                    if out.len() > MAX_ENTRY {
                        return fail("an entry expands beyond 256 MiB");
                    }
                }
            }
            _ => return fail("invalid block type"),
        }
        if out.len() > MAX_ENTRY {
            return fail("an entry expands beyond 256 MiB");
        }
        if last {
            return Ok(out);
        }
    }
}

fn u16_at(data: &[u8], at: usize) -> Result<u16, ZipError> {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| ZipError("archive ends early".into()))
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, ZipError> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| ZipError("archive ends early".into()))
}

/// One entry of the central directory.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    local_header: usize,
}

/// A ZIP archive in memory.
pub struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

impl<'a> Archive<'a> {
    /// Reads the archive's central directory.
    pub fn new(data: &'a [u8]) -> Result<Self, ZipError> {
        // The end of central directory record is the last 22 bytes, unless a comment
        // (up to 65535 bytes) follows it.
        let earliest = data.len().saturating_sub(22 + 65535);
        let end = (earliest..=data.len().saturating_sub(22))
            .rev()
            .find(|&i| data[i..i + 4] == [0x50, 0x4B, 0x05, 0x06])
            .ok_or_else(|| ZipError("not a ZIP archive (no end of central directory)".into()))?;
        let count = usize::from(u16_at(data, end + 10)?);
        let mut at = u32_at(data, end + 16)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(data, at)? != 0x0201_4B50 {
                return fail("damaged central directory");
            }
            let name_length = usize::from(u16_at(data, at + 28)?);
            let extra = usize::from(u16_at(data, at + 30)?);
            let comment = usize::from(u16_at(data, at + 32)?);
            let name = data
                .get(at + 46..at + 46 + name_length)
                .ok_or_else(|| ZipError("archive ends early".into()))?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                flags: u16_at(data, at + 8)?,
                method: u16_at(data, at + 10)?,
                crc: u32_at(data, at + 16)?,
                compressed: u32_at(data, at + 20)? as usize,
                size: u32_at(data, at + 24)? as usize,
                local_header: u32_at(data, at + 42)? as usize,
            });
            at += 46 + name_length + extra + comment;
        }
        Ok(Self { data, entries })
    }

    /// The names of its entries.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// An entry's contents, checked against its CRC-32.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, ZipError> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| ZipError(format!("no `{name}` in the archive")))?;
        if entry.flags & 1 != 0 {
            return fail(format!("`{name}` is encrypted"));
        }
        let at = entry.local_header;
        if u32_at(self.data, at)? != 0x0403_4B50 {
            return fail(format!("damaged local header for `{name}`"));
        }
        let start = at
            + 30
            + usize::from(u16_at(self.data, at + 26)?)
            + usize::from(u16_at(self.data, at + 28)?);
        let raw = self
            .data
            .get(start..start + entry.compressed)
            .ok_or_else(|| ZipError(format!("`{name}` ends early")))?;
        let contents = match entry.method {
            0 => raw.to_vec(),
            8 => inflate(raw)?,
            other => return fail(format!("`{name}` uses compression method {other}")),
        };
        if contents.len() != entry.size || crc32(&contents) != entry.crc {
            return fail(format!("`{name}` fails its size or CRC-32 check"));
        }
        Ok(contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        // The standard check value of CRC-32 for the ASCII digits "123456789" is
        // 0xCBF43926 (ITU-T V.42 / ISO 3309 CRC, as used by ZIP).
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn stored_and_fixed_blocks() {
        // A stored block: header bits 1 (last), 00 (stored), then LEN, NLEN, bytes.
        let stored = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&stored).expect("stored"), b"abc");
        // A fixed-Huffman block, built by hand from RFC 1951 3.2.6: "a" (0x61 → code
        // 0x30 + 0x61 = 10010001, 8 bits), then a match of length 3 at distance 1
        // (length code 257 → 0000001, distance code 0 → 00000), then end of block (256 →
        // 0000000). Bits are packed least significant first; Huffman codes most
        // significant first.
        let mut bits: Vec<u8> = vec![1, 1, 0]; // last block, type 01 (LSB first: 1, 0)
        let push_code = |bits: &mut Vec<u8>, code: u32, len: u32| {
            for i in (0..len).rev() {
                bits.push(((code >> i) & 1) as u8);
            }
        };
        push_code(&mut bits, 0x30 + 0x61, 8);
        push_code(&mut bits, 1, 7); // length code 257 = length 3
        push_code(&mut bits, 0, 5); // distance code 0 = distance 1
        push_code(&mut bits, 0, 7); // end of block
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|c| c.iter().enumerate().fold(0u8, |b, (i, &v)| b | (v << i)))
            .collect();
        assert_eq!(inflate(&bytes).expect("fixed"), b"aaaa");
        assert!(inflate(&[0x07]).is_err(), "block type 11 is invalid");
        assert!(
            inflate(&[0x01, 0x03, 0x00, 0x00, 0x00]).is_err(),
            "LEN/NLEN mismatch"
        );
    }

    #[test]
    fn dynamic_huffman_block() {
        // 38 spreadsheet rows compressed with dynamic Huffman codes (block type 2), the
        // kind spreadsheet files use; the bytes were made once with Python 3's zlib
        // module (level 9, raw Deflate). What they must decompress to is the text itself.
        let compressed: &[u8] = &[
            0x75, 0xd5, 0x4b, 0x4e, 0x03, 0x41, 0x10, 0x03, 0xd0, 0xab, 0x20, 0x0e, 0x40, 0xa6,
            0xec, 0x9e, 0x9f, 0x34, 0x64, 0x83, 0x38, 0x09, 0x07, 0x40, 0x62, 0x01, 0xd7, 0xe7,
            0xd3, 0x9e, 0xae, 0xa8, 0x82, 0x77, 0x51, 0x5b, 0xb1, 0x6a, 0xf1, 0xe4, 0x39, 0x3e,
            0xde, 0xbf, 0x1e, 0x3e, 0x9e, 0x1f, 0xf1, 0x78, 0x3d, 0xde, 0x7e, 0x7f, 0xbc, 0xfc,
            0xfe, 0xfa, 0xbc, 0xce, 0x13, 0x8e, 0xcb, 0xe7, 0xf5, 0xb8, 0xbc, 0xe9, 0xfd, 0xb5,
            0xbf, 0xf3, 0x69, 0x1a, 0xef, 0x97, 0x9f, 0xff, 0x5e, 0x0f, 0x15, 0x70, 0x14, 0x50,
            0x05, 0x2c, 0x05, 0xfd, 0xbd, 0x3d, 0xcd, 0xff, 0x17, 0xb4, 0x51, 0xd0, 0x54, 0xd0,
            0x4a, 0x41, 0x7f, 0x5f, 0xdc, 0x05, 0xf3, 0x28, 0x98, 0x55, 0x30, 0x97, 0x82, 0xfe,
            0xbe, 0xba, 0x0b, 0x96, 0x51, 0xb0, 0xa8, 0x60, 0x29, 0x05, 0xfd, 0x7d, 0x77, 0x17,
            0xac, 0xa3, 0x60, 0x55, 0xc1, 0x5a, 0x0a, 0xfa, 0x7b, 0x4c, 0xee, 0x84, 0x6d, 0x34,
            0x6c, 0x6a, 0xd8, 0x4a, 0x43, 0x7f, 0x0f, 0xb8, 0x1b, 0xf6, 0xd1, 0xb0, 0xab, 0x61,
            0x2f, 0x0d, 0xfd, 0x3d, 0xe8, 0x6e, 0x88, 0x69, 0x54, 0xfc, 0xfd, 0xfc, 0xe9, 0x88,
            0xa9, 0x74, 0x28, 0x88, 0xd9, 0x9d, 0x11, 0x91, 0x25, 0xa1, 0x92, 0xa8, 0x25, 0x3d,
            0x88, 0xc5, 0x5e, 0x92, 0x2a, 0x43, 0x2c, 0xa3, 0xb2, 0x54, 0x10, 0x9b, 0xbd, 0x24,
            0x65, 0x86, 0x68, 0x46, 0xa5, 0xa9, 0x20, 0x76, 0x7b, 0x49, 0xea, 0x0c, 0xf1, 0x8c,
            0xca, 0x53, 0x01, 0xc2, 0x5e, 0x92, 0x42, 0x43, 0x44, 0xa3, 0x12, 0x55, 0x00, 0xd8,
            0x4b, 0x52, 0x69, 0x88, 0x69, 0x54, 0xa6, 0x0a, 0xd0, 0xec, 0x25, 0x29, 0x35, 0x44,
            0x35, 0x2a, 0x55, 0x05, 0x98, 0xed, 0x25, 0x89, 0x35, 0xa4, 0x35, 0xaa, 0x56, 0x05,
            0x58, 0xed, 0x25, 0xe9, 0x35, 0x04, 0x36, 0x2a, 0x58, 0x05, 0xd8, 0xdc, 0x25, 0x48,
            0xb1, 0x90, 0x58, 0x54, 0xb1, 0x0a, 0x38, 0xb9, 0x4b, 0x90, 0x62, 0x21, 0xb1, 0xa8,
            0x62, 0x15, 0x30, 0xec, 0x25, 0x37, 0x3b, 0x2a, 0xb1, 0xb8, 0x1b, 0x52, 0x2d, 0xa9,
            0x9d, 0x52, 0xa4, 0x58, 0x48, 0x2c, 0xaa, 0x58, 0x05, 0xb4, 0x73, 0x8a, 0x14, 0x0b,
            0x89, 0x45, 0x15, 0xab, 0x80, 0x76, 0x52, 0x91, 0x62, 0x21, 0xb1, 0xa8, 0x62, 0x15,
            0xd0, 0xce, 0x2a, 0x52, 0x2c, 0x24, 0x16, 0x55, 0xac, 0x02, 0xda, 0x69, 0x45, 0x8a,
            0x85, 0xc4, 0xa2, 0x8a, 0x55, 0xd0, 0xec, 0xba, 0x22, 0xc5, 0x42, 0x62, 0x51, 0xc5,
            0x2a, 0x68, 0x76, 0x60, 0x91, 0x62, 0x21, 0xb1, 0xa8, 0x62, 0x15, 0x34, 0xbb, 0xb1,
            0x4c, 0xb1, 0x94, 0x58, 0x56, 0xb1, 0x0a, 0x9a, 0xdd, 0x58, 0xa6, 0x58, 0x4a, 0x2c,
            0xab, 0x58, 0x05, 0xcd, 0x6e, 0x2c, 0x53, 0x2c, 0x25, 0x96, 0x55, 0xac, 0x82, 0x66,
            0x37, 0x96, 0x37, 0x5f, 0x7f, 0x89, 0xe5, 0xdd, 0xe7, 0x5f, 0xdf, 0x7f, 0xbb, 0xb1,
            0x4c, 0xb1, 0x94, 0x58, 0x56, 0xb1, 0x67, 0x60, 0x37, 0x96, 0x29, 0x96, 0x12, 0xcb,
            0x2a, 0xf6, 0x0c, 0xec, 0xc6, 0x32, 0xc5, 0x52, 0x62, 0x59, 0xc5, 0x9e, 0x81, 0xdd,
            0x58, 0xa6, 0x58, 0x4a, 0x2c, 0xab, 0xd8, 0x33, 0xb0, 0x1b, 0xcb, 0x14, 0x4b, 0x89,
            0x65, 0x15, 0x7b, 0x06, 0x76, 0x63, 0x99, 0x62, 0x29, 0xb1, 0xac, 0x62, 0xcf, 0xe0,
            0x6e, 0x63, 0xbf, 0x01,
        ];
        assert_eq!((compressed[0] >> 1) & 3, 2, "a dynamic block");
        let text: String = (2..40)
            .map(|i| {
                format!(
                    "<row r=\"{i}\"><c r=\"C{i}\"><v>{}</v></c><c r=\"E{i}\"><v>{:.1}</v></c></row>",
                    500 + i,
                    f64::from(i) * 1.5
                )
            })
            .collect();
        assert_eq!(inflate(compressed).expect("inflates"), text.as_bytes());
        // Cut short, it is an error, not a shorter result.
        assert!(inflate(&compressed[..compressed.len() / 2]).is_err());
    }

    #[test]
    fn rejects_what_is_not_an_archive() {
        assert!(Archive::new(b"not a zip").is_err());
    }
}
