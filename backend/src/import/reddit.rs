use super::*;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::{cell::Cell, io::Read, rc::Rc};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const COMMENTS_HEADER: [&str; 6] = ["id", "permalink", "date", "subreddit", "parent", "body"];
const POSTS_HEADER: [&str; 7] = ["id", "permalink", "date", "subreddit", "title", "url", "body"];

fn date(value: &str) -> Result<DateTime<Utc>, ImportError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S UTC").map(|date| date.and_utc()))
        .map_err(|_| ImportErrorCode::InvalidRecord.into())
}

fn clean(value: &str) -> Result<String, ImportError> {
    if value.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    Ok(value.replace("\r\n", "\n").replace('\r', "\n").nfc().collect())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Raw-byte ceiling for one CSV record (quotes, separators, and doubled
/// quotes included). The csv crate grows its record buffer without bound, so
/// this guard caps memory; the exact limit is still enforced per record on the
/// decoded fields (`MAX_CSV_RECORD_BYTES`).
const MAX_RAW_CSV_RECORD_BYTES: usize = 2 * limits::MAX_CSV_RECORD_BYTES + 65_536;

#[derive(Default)]
struct QuoteState {
    odd_quotes: Cell<bool>,
    oversized: Cell<bool>,
}

struct QuoteReader<R> {
    inner: R,
    state: Rc<QuoteState>,
    record_bytes: usize,
}

impl<R: Read> Read for QuoteReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        for byte in &buffer[..read] {
            match *byte {
                b'"' => self.state.odd_quotes.set(!self.state.odd_quotes.get()),
                b'\n' | b'\r' if !self.state.odd_quotes.get() => { self.record_bytes = 0; continue; }
                _ => {}
            }
            self.record_bytes += 1;
            if self.record_bytes > MAX_RAW_CSV_RECORD_BYTES {
                self.state.oversized.set(true);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "CSV record too large"));
            }
        }
        Ok(read)
    }
}

fn permalink_id(permalink: &str, is_comment: bool) -> Option<&str> {
    let segments: Vec<_> = permalink.split('/').filter(|segment| !segment.is_empty()).collect();
    let comments = segments.iter().position(|segment| *segment == "comments")?;
    if is_comment { segments.last().copied() } else { segments.get(comments + 1).copied() }
}

/// Streams `file` row by row, handing each normalized record to `emit`.
/// Returns the number of records emitted. Nothing is accumulated here.
pub fn parse_csv<R: Read>(
    reader: R,
    file: &str,
    import_id: Uuid,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    let expected = match file {
        "comments.csv" => &COMMENTS_HEADER[..],
        "posts.csv" => &POSTS_HEADER[..],
        _ => return Ok(0),
    };
    let is_comment = file == "comments.csv";
    let state = Rc::new(QuoteState::default());
    let checked = QuoteReader { inner: reader, state: Rc::clone(&state), record_bytes: 0 };
    let mut csv = csv::ReaderBuilder::new().has_headers(true).flexible(false).from_reader(checked);
    let row_error = |state: &QuoteState, otherwise: ImportErrorCode| -> ImportError {
        if state.oversized.get() { ImportErrorCode::ResourceLimit.into() } else { otherwise.into() }
    };
    let headers = csv.headers().map_err(|_| row_error(&state, ImportErrorCode::InvalidSchema))?.clone();
    if headers.as_byte_record().as_slice().len() > limits::MAX_CSV_HEADER_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    let first = headers.get(0).unwrap_or("").trim_start_matches('\u{feff}');
    let header_names: Vec<&str> = std::iter::once(first).chain(headers.iter().skip(1)).collect();
    if expected.iter().any(|name| !header_names.iter().any(|candidate| candidate == name)) {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    let exact_header = header_names == expected;
    let index = |name: &str| header_names.iter().position(|value| *value == name).unwrap();
    let mut emitted = 0usize;
    for (ordinal, row) in csv.records().enumerate() {
        if emitted >= limits::MAX_NORMALIZED_RECORDS {
            return Err(ImportErrorCode::ResourceLimit.into());
        }
        let row = row.map_err(|_| row_error(&state, ImportErrorCode::InvalidRecord))?;
        if row.as_byte_record().as_slice().len() > limits::MAX_CSV_RECORD_BYTES {
            return Err(ImportErrorCode::ResourceLimit.into());
        }
        let id = row.get(index("id")).unwrap();
        if !valid_id(id) {
            return Err(ImportErrorCode::InvalidRecord.into());
        }
        if permalink_id(row.get(index("permalink")).unwrap(), is_comment).is_some_and(|permalink_id| permalink_id != id) {
            return Err(ImportErrorCode::InvalidRecord.into());
        }
        let raw_body = row.get(index("body")).unwrap();
        let title_sentinel = (!is_comment).then(|| row.get(index("title")).unwrap())
            .filter(|title| matches!(*title, "[deleted]" | "[deleted by user]" | "[removed]"));
        let state = if raw_body == "[deleted]" || title_sentinel.is_some_and(|title| matches!(title, "[deleted]" | "[deleted by user]")) {
            ContentState::Deleted
        } else if raw_body == "[removed]" || title_sentinel == Some("[removed]") {
            ContentState::Removed
        } else {
            ContentState::Active
        };
        let body = if state == ContentState::Active {
            let body = clean(raw_body)?;
            if body.trim().is_empty() { return Err(ImportErrorCode::InvalidRecord.into()); }
            Some(body)
        } else { None };
        let title = if !is_comment && state == ContentState::Active {
            let title = clean(row.get(index("title")).unwrap())?;
            if title.trim().is_empty() { return Err(ImportErrorCode::InvalidRecord.into()); }
            Some(title)
        } else { None };
        let parent_source_id = if is_comment {
            let parent = row.get(index("parent")).unwrap()
                .strip_prefix("t1_").or_else(|| row.get(index("parent")).unwrap().strip_prefix("t3_"))
                .filter(|value| valid_id(value))
                .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
            Some(parent.to_owned())
        } else { None };
        emitted += 1;
        emit(NormalizedArchiveRecord {
            schema_version: 1, import_id, platform: ArchivePlatform::Reddit,
            source_logical_id: id.into(), source_revision_id: id.into(),
            record_type: if is_comment { RecordType::Comment } else { RecordType::Post },
            text_format: TextFormat::Standard, authorship: Authorship::OwnerAuthored, state,
            created_at: Some(date(row.get(index("date")).unwrap())?), title, body, parent_source_id,
            quoted_source_id: None,
            relation_confidence: if is_comment { RelationConfidence::Explicit } else { RelationConfidence::None },
            text_truncated: false,
            provenance: RecordProvenance {
                source_file: file.into(), source_ordinal: (ordinal + 1) as u64,
                format_family: FormatFamily::RedditGdprCsv,
                format_confidence: if is_comment && exact_header { FormatConfidence::Confirmed }
                    else if is_comment { FormatConfidence::Compatible } else { FormatConfidence::Provisional },
            },
        })?;
    }
    drop(csv);
    if state.odd_quotes.get() {
        return Err(ImportErrorCode::InvalidRecord.into());
    }
    Ok(emitted)
}
