use super::{limits::*, ArchivePlatform, ImportError, ImportErrorCode};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read},
};
use zip::{CompressionMethod, ZipArchive};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone)]
pub struct SelectedEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

pub struct SafeArchive {
    bytes: Vec<u8>,
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

fn has_exact_eocd(bytes: &[u8]) -> bool {
    let Some(offset) = bytes.windows(4).rposition(|window| window == b"PK\x05\x06") else {
        return false;
    };
    if offset + 22 > bytes.len() {
        return false;
    }
    let comment_len = u16::from_le_bytes([bytes[offset + 20], bytes[offset + 21]]) as usize;
    offset + 22 + comment_len == bytes.len()
}

#[derive(Clone)]
struct CentralEntry {
    name: String,
    flags: u16,
    method: u16,
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn central_entries(bytes: &[u8]) -> Result<Vec<CentralEntry>, ImportError> {
    let eocd = bytes.windows(4).rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
    let count = u16_at(bytes, eocd + 10).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
    if count > MAX_ZIP_ENTRIES {
        return Err(ImportErrorCode::ZipLimitExceeded.into());
    }
    let mut offset = u32_at(bytes, eocd + 16).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
    let mut entries = Vec::with_capacity(count);
    let mut canonical_names = HashSet::with_capacity(count);
    for _ in 0..count {
        if bytes.get(offset..offset + 4) != Some(b"PK\x01\x02") {
            return Err(ImportErrorCode::UnsupportedZip.into());
        }
        let flags = u16_at(bytes, offset + 8).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        let method = u16_at(bytes, offset + 10).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        let name_len = u16_at(bytes, offset + 28).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
        let extra_len = u16_at(bytes, offset + 30).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
        let comment_len = u16_at(bytes, offset + 32).ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))? as usize;
        let raw_name = bytes.get(offset + 46..offset + 46 + name_len)
            .ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        let name = std::str::from_utf8(raw_name)
            .map_err(|_| ImportError::from(ImportErrorCode::UnsafeZipEntry))?.to_owned();
        if !safe_path(&name) || name.to_ascii_lowercase().ends_with(".zip") {
            return Err(ImportErrorCode::UnsafeZipEntry.into());
        }
        let canonical: String = name.nfc().collect();
        if !canonical_names.insert(canonical) {
            return Err(ImportErrorCode::DuplicateZipEntry.into());
        }
        entries.push(CentralEntry { name, flags, method });
        offset = offset.checked_add(46 + name_len + extra_len + comment_len)
            .ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedZip))?;
    }
    Ok(entries)
}

impl SafeArchive {
    pub fn index(bytes: Vec<u8>, platform: ArchivePlatform) -> Result<Self, ImportError> {
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(ImportErrorCode::UploadTooLarge.into());
        }
        if !matches!(
            bytes.get(0..4),
            Some(b"PK\x03\x04") | Some(b"PK\x05\x06") | Some(b"PK\x07\x08")
        ) {
            return Err(ImportErrorCode::NotZip.into());
        }
        if !has_exact_eocd(&bytes) {
            return Err(ImportErrorCode::UnsupportedZip.into());
        }
        let central = central_entries(&bytes)?;
        let has_reddit = central.iter().any(|entry| allowed(ArchivePlatform::Reddit, &entry.name));
        let has_x = central.iter().any(|entry| allowed(ArchivePlatform::X, &entry.name));
        if has_reddit && has_x {
            return Err(ImportErrorCode::AmbiguousArchive.into());
        }
        let declared_present = match platform {
            ArchivePlatform::Reddit => has_reddit,
            ArchivePlatform::X => has_x,
        };
        let other_present = match platform {
            ArchivePlatform::Reddit => has_x,
            ArchivePlatform::X => has_reddit,
        };
        if !declared_present && other_present {
            return Err(ImportErrorCode::PlatformMismatch.into());
        }
        for entry in central.iter().filter(|entry| allowed(platform, &entry.name)) {
            if entry.flags & 1 != 0 {
                return Err(ImportErrorCode::EncryptedZip.into());
            }
            if !matches!(entry.method, 0 | 8) {
                return Err(ImportErrorCode::UnsupportedCompression.into());
            }
        }
        let mut zip = ZipArchive::new(Cursor::new(&bytes))
            .map_err(|_| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        if zip.len() > MAX_ZIP_ENTRIES {
            return Err(ImportErrorCode::ZipLimitExceeded.into());
        }
        let mut names = HashSet::new();
        let mut selected = Vec::new();
        let mut total_uncompressed = 0u64;
        let mut total_compressed = 0u64;
        for index in 0..zip.len() {
            let file = zip
                .by_index_raw(index)
                .map_err(|_| ImportError::from(ImportErrorCode::UnsupportedZip))?;
            let name = std::str::from_utf8(file.name_raw())
                .map_err(|_| ImportError::from(ImportErrorCode::UnsafeZipEntry))?
                .to_owned();
            if !safe_path(&name)
                || name.to_ascii_lowercase().ends_with(".zip")
                || file.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(ImportErrorCode::UnsafeZipEntry.into());
            }
            if !names.insert(name.clone()) {
                return Err(ImportErrorCode::DuplicateZipEntry.into());
            }
            if allowed(platform, &name) {
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
                    name,
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
        drop(zip);
        Ok(Self { bytes, selected })
    }

    pub fn read_selected(&self) -> Result<BTreeMap<String, Vec<u8>>, ImportError> {
        let mut zip = ZipArchive::new(Cursor::new(&self.bytes))
            .map_err(|_| ImportError::from(ImportErrorCode::UnsupportedZip))?;
        let mut output = BTreeMap::new();
        for entry in &self.selected {
            let file = zip
                .by_name(&entry.name)
                .map_err(|_| ImportError::from(ImportErrorCode::IntegrityMismatch))?;
            let mut bytes = Vec::with_capacity(entry.uncompressed_size.min(1024 * 1024) as usize);
            file.take(entry.uncompressed_size + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ImportError::from(ImportErrorCode::IntegrityMismatch))?;
            if bytes.len() as u64 != entry.uncompressed_size {
                return Err(ImportErrorCode::IntegrityMismatch.into());
            }
            output.insert(entry.name.clone(), bytes);
        }
        Ok(output)
    }
}
