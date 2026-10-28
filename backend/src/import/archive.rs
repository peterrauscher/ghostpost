//! Safe ZIP indexing over any `Read + Seek` source (the spooled temp file in
//! production, a `Cursor` in tests). Nothing here loads the archive or an
//! entry into memory: the EOCD comes from a bounded tail read, the central
//! directory is walked header by header, and selected entries are handed to
//! parsers as streaming readers.
use super::{limits::*, ArchivePlatform, ImportError, ImportErrorCode};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{self, BufReader, Read, Seek, SeekFrom},
};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

#[derive(Debug, Clone)]
pub struct SelectedEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

pub struct SafeArchive<R: Read + Seek> {
    zip: ZipArchive<R>,
    pub selected: Vec<SelectedEntry>,
}

fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ENTRY_NAME_BYTES
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && {
            let segments: Vec<_> = name.split('/').collect();
            segments.len() <= MAX_PATH_SEGMENTS
                && segments
                    .iter()
                    .all(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        }
}

fn allowed(platform: ArchivePlatform, name: &str) -> bool {
    match platform {
        ArchivePlatform::Reddit => matches!(
            name,
            "posts.csv" | "comments.csv" | "statistics.csv" | "checkfile.csv"
        ),
        ArchivePlatform::X => {
            matches!(
                name,
                "data/manifest.js"
                    | "data/tweets.js"
                    | "data/tweet.js"
                    | "data/note-tweet.js"
                    | "data/deleted-tweets.js"
            ) || valid_numbered(name, "data/tweets-part", ".js")
                || valid_numbered(name, "data/tweet-part", ".js")
                || valid_classic_month(name)
        }
    }
}

fn part_number(name: &str) -> Option<u32> {
    ["data/tweets-part", "data/tweet-part"].into_iter().find_map(|prefix| {
        let value = name.strip_prefix(prefix)?.strip_suffix(".js")?;
        (!value.is_empty() && value.len() <= 4 && value.bytes().all(|b| b.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
    })
}

fn valid_numbered(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .is_some_and(|value| {
            !value.is_empty() && value.len() <= 4 && value.bytes().all(|b| b.is_ascii_digit())
        })
}

fn valid_classic_month(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("data/tweets/")
        .and_then(|value| value.strip_suffix(".js"))
    else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() == 7
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'_'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && matches!(&stem[5..7], "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12")
}

fn unsupported(_: io::Error) -> ImportError {
    ImportErrorCode::UnsupportedZip.into()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

/// 128-bit digest of a name: duplicate detection over up to `MAX_ZIP_ENTRIES`
/// names without keeping every name in memory.
fn name_digest(name: &str) -> [u8; 16] {
    let digest = Sha256::digest(name.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

struct Eocd {
    count: usize,
    central_offset: u64,
}

/// The EOCD record must end the file exactly (comment included), so it lies
/// within the last 22 + 65535 bytes; only that window is read.
fn read_exact_eocd<R: Read + Seek>(reader: &mut R, len: u64) -> Result<Eocd, ImportError> {
    const EOCD_LEN: usize = 22;
    let window = len.min((EOCD_LEN + u16::MAX as usize) as u64);
    reader.seek(SeekFrom::Start(len - window)).map_err(unsupported)?;
    let mut tail = vec![0u8; window as usize];
    reader.read_exact(&mut tail).map_err(unsupported)?;
    let offset = tail
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
    if offset + EOCD_LEN > tail.len() {
        return Err(ImportErrorCode::UnsupportedZip.into());
    }
    let comment_len = u16_at(&tail, offset + 20).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
    if offset + EOCD_LEN + comment_len != tail.len() {
        return Err(ImportErrorCode::UnsupportedZip.into());
    }
    let count = u16_at(&tail, offset + 10).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
    let central_offset = u32_at(&tail, offset + 16).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as u64;
    Ok(Eocd { count, central_offset })
}

struct CentralEntry {
    name: String,
    flags: u16,
    method: u16,
}

struct CentralScan {
    has_reddit: bool,
    has_x: bool,
    /// Only entries allowlisted for either platform; everything else is
    /// validated and forgotten.
    allowlisted: Vec<CentralEntry>,
}

/// Walks the central directory header by header from the raw bytes, before
/// the zip crate parses it, so unsafe names, NFC duplicates, encryption flags,
/// and compression methods are rejected without trusting a library's view.
fn scan_central<R: Read + Seek>(reader: &mut R, eocd: &Eocd) -> Result<CentralScan, ImportError> {
    if eocd.count > MAX_ZIP_ENTRIES {
        return Err(ImportErrorCode::ZipLimitExceeded.into());
    }
    reader.seek(SeekFrom::Start(eocd.central_offset)).map_err(unsupported)?;
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut canonical_names = HashSet::with_capacity(eocd.count);
    let mut scan = CentralScan { has_reddit: false, has_x: false, allowlisted: Vec::new() };
    let mut header = [0u8; 46];
    let mut raw_name = Vec::with_capacity(MAX_ENTRY_NAME_BYTES);
    for _ in 0..eocd.count {
        reader.read_exact(&mut header).map_err(unsupported)?;
        if &header[0..4] != b"PK\x01\x02" {
            return Err(ImportErrorCode::UnsupportedZip.into());
        }
        let flags = u16_at(&header, 8).expect("fixed header");
        let method = u16_at(&header, 10).expect("fixed header");
        let name_len = u16_at(&header, 28).expect("fixed header") as usize;
        let extra_len = u16_at(&header, 30).expect("fixed header") as i64;
        let comment_len = u16_at(&header, 32).expect("fixed header") as i64;
        raw_name.resize(name_len, 0);
        reader.read_exact(&mut raw_name).map_err(unsupported)?;
        reader.seek_relative(extra_len + comment_len).map_err(unsupported)?;
        let name = std::str::from_utf8(&raw_name)
            .map_err(|_| ImportError::from(ImportErrorCode::UnsafeZipEntry))?;
        if !safe_path(name) || name.to_ascii_lowercase().ends_with(".zip") {
            return Err(ImportErrorCode::UnsafeZipEntry.into());
        }
        let canonical: String = name.nfc().collect();
        if !canonical_names.insert(name_digest(&canonical)) {
            return Err(ImportErrorCode::DuplicateZipEntry.into());
        }
        let reddit = allowed(ArchivePlatform::Reddit, name);
        let x = allowed(ArchivePlatform::X, name);
        scan.has_reddit |= reddit;
        scan.has_x |= x;
        if reddit || x {
            scan.allowlisted.push(CentralEntry { name: name.to_owned(), flags, method });
        }
    }
    Ok(scan)
}

/// Counts decompressed bytes and remembers whether the underlying ZIP stream
/// failed, so a CRC or inflate error is reported as `IntegrityMismatch` even
/// when a parser surfaces it as its own error.
struct VerifiedReader<R> {
    inner: R,
    limit: u64,
    read: u64,
    io_failed: bool,
}

impl<R: Read> Read for VerifiedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buffer) {
            Ok(n) => {
                self.read += n as u64;
                if self.read > self.limit {
                    self.io_failed = true;
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "entry longer than its central size"));
                }
                Ok(n)
            }
            Err(err) => {
                self.io_failed = true;
                Err(err)
            }
        }
    }
}

impl<R: Read + Seek> SafeArchive<R> {
    pub fn index(mut reader: R, platform: ArchivePlatform) -> Result<Self, ImportError> {
        let len = reader.seek(SeekFrom::End(0)).map_err(unsupported)?;
        if len > MAX_ARCHIVE_BYTES {
            return Err(ImportErrorCode::UploadTooLarge.into());
        }
        let mut magic = [0u8; 4];
        reader.seek(SeekFrom::Start(0)).map_err(unsupported)?;
        if len < 4 || reader.read_exact(&mut magic).is_err()
            || !matches!(&magic, b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
        {
            return Err(ImportErrorCode::NotZip.into());
        }
        let eocd = read_exact_eocd(&mut reader, len)?;
        let central = scan_central(&mut reader, &eocd)?;
        if central.has_reddit && central.has_x {
            return Err(ImportErrorCode::AmbiguousArchive.into());
        }
        let (declared_present, other_present) = match platform {
            ArchivePlatform::Reddit => (central.has_reddit, central.has_x),
            ArchivePlatform::X => (central.has_x, central.has_reddit),
        };
        if !declared_present && other_present {
            return Err(ImportErrorCode::PlatformMismatch.into());
        }
        for entry in central.allowlisted.iter().filter(|entry| allowed(platform, &entry.name)) {
            if entry.flags & 1 != 0 {
                return Err(ImportErrorCode::EncryptedZip.into());
            }
            if !matches!(entry.method, 0 | 8) {
                return Err(ImportErrorCode::UnsupportedCompression.into());
            }
        }
        drop(central);
        reader.seek(SeekFrom::Start(0)).map_err(unsupported)?;
        let mut zip = ZipArchive::new(reader)
            .map_err(|_| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        if zip.len() > MAX_ZIP_ENTRIES {
            return Err(ImportErrorCode::ZipLimitExceeded.into());
        }
        let mut names = HashSet::with_capacity(zip.len());
        let mut selected = Vec::new();
        let mut total_uncompressed = 0u64;
        let mut total_compressed = 0u64;
        for index in 0..zip.len() {
            let file = zip
                .by_index_raw(index)
                .map_err(|_| ImportError::from(ImportErrorCode::UnsupportedZip))?;
            let name = std::str::from_utf8(file.name_raw())
                .map_err(|_| ImportError::from(ImportErrorCode::UnsafeZipEntry))?;
            if !safe_path(name)
                || name.to_ascii_lowercase().ends_with(".zip")
                || file.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(ImportErrorCode::UnsafeZipEntry.into());
            }
            if !names.insert(name_digest(name)) {
                return Err(ImportErrorCode::DuplicateZipEntry.into());
            }
            if allowed(platform, name) {
                if file.encrypted() {
                    return Err(ImportErrorCode::EncryptedZip.into());
                }
                if selected.len() >= MAX_SELECTED_ENTRIES
                    || file.size() > MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES
                {
                    return Err(ImportErrorCode::ZipLimitExceeded.into());
                }
                if !matches!(file.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
                    return Err(ImportErrorCode::UnsupportedCompression.into());
                }
                let compressed = file.compressed_size();
                if (compressed == 0 && file.size() > 0)
                    || (compressed > 0 && file.size() / compressed > MAX_SELECTED_ENTRY_RATIO)
                {
                    return Err(ImportErrorCode::ZipLimitExceeded.into());
                }
                total_uncompressed = total_uncompressed.saturating_add(file.size());
                total_compressed = total_compressed.saturating_add(compressed);
                selected.push(SelectedEntry {
                    name: name.to_owned(),
                    compressed_size: compressed,
                    uncompressed_size: file.size(),
                });
            }
        }
        if platform == ArchivePlatform::X {
            let mut parts: Vec<u32> = selected
                .iter()
                .filter_map(|entry| part_number(&entry.name))
                .collect();
            if parts.len() > 1 {
                parts.sort_unstable();
                if parts.iter().copied().ne(0..parts.len() as u32) {
                    return Err(ImportErrorCode::UnsupportedFormatVersion.into());
                }
                selected.sort_by_key(|entry| part_number(&entry.name).unwrap_or(u32::MAX));
            }
        }
        if selected.is_empty() {
            return Err(ImportErrorCode::NoSupportedContent.into());
        }
        if total_uncompressed > MAX_SELECTED_TOTAL_UNCOMPRESSED_BYTES
            || (total_compressed > 0
                && total_uncompressed / total_compressed > MAX_SELECTED_AGGREGATE_RATIO)
        {
            return Err(ImportErrorCode::ZipLimitExceeded.into());
        }
        if platform != ArchivePlatform::X || !selected.iter().any(|entry| part_number(&entry.name).is_some()) {
            selected.sort_by(|left, right| left.name.cmp(&right.name));
        }
        Ok(Self { zip, selected })
    }

    /// Streams one selected entry into `parse`. The reader yields exactly the
    /// central-directory size; a short, long, or corrupt entry (CRC/inflate
    /// failure) is `IntegrityMismatch` whatever error the parser reports.
    pub fn read_entry<T>(
        &mut self,
        name: &str,
        parse: impl FnOnce(&mut dyn Read) -> Result<T, ImportError>,
    ) -> Result<T, ImportError> {
        let expected = self
            .selected
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.uncompressed_size)
            .ok_or_else(|| ImportError::from(ImportErrorCode::IntegrityMismatch))?;
        let file = self
            .zip
            .by_name(name)
            .map_err(|_| ImportError::from(ImportErrorCode::IntegrityMismatch))?;
        let mut reader = VerifiedReader { inner: file, limit: expected, read: 0, io_failed: false };
        let result = parse(&mut reader);
        if reader.io_failed {
            return Err(ImportErrorCode::IntegrityMismatch.into());
        }
        let value = result?;
        // Drain what the parser did not need so the CRC is always checked.
        io::copy(&mut reader, &mut io::sink())
            .map_err(|_| ImportError::from(ImportErrorCode::IntegrityMismatch))?;
        if reader.io_failed || reader.read != expected {
            return Err(ImportErrorCode::IntegrityMismatch.into());
        }
        Ok(value)
    }
}
