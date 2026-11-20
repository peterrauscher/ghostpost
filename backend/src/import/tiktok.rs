//! TikTok data export parsing.
//!
//! Authored comments are read only from the top-level `Comment` section and
//! authored posts only from the top-level `Post` or `Video` section. Known
//! singular/plural wrapper variants and case/punctuation variants of those
//! keys are accepted. Direct messages, profile data, watch history, likes,
//! favorites, ads, shop data, and recently deleted posts are never traversed.
//!
//! JSON is streamed: each list element is buffered alone under
//! `MAX_JSON_ELEMENT_BYTES`, unknown sections are skipped without buffering,
//! and `MAX_JSON_NESTING` is enforced while reading. TXT exports are read one
//! bounded line at a time and only `Comments.txt`, `Videos.txt`, and
//! `Posts.txt` (with the matching folder) are recognized. Multiline values
//! continue until the next recognized label or the next `Date:` record.
//! Known metadata labels (photo, likes, sound, location, and the other export
//! columns) are never joined to authored text. A colon-bearing line with an
//! unknown label is ambiguous inside an active text field: the format has no
//! escaping, so it is treated as text while known labels are not.
use super::*;
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

/// Keys are short labels, not content. Anything larger is malformed.
const MAX_JSON_KEY_BYTES: usize = 4_096;
/// Landing page or download links are far below this; the cap prevents a
/// malformed record from retaining an unbounded URL.
const MAX_LINK_BYTES: usize = 8_192;
/// One TXT line may hold a complete text field plus line ending headroom.
/// The final text field and record caps are enforced after continuation.
const MAX_TXT_LINE_BYTES: usize = limits::MAX_TEXT_FIELD_BYTES + 65_536;

const DATE_FIELDS: &[&str] = &["date", "createdat", "createtime", "creationtime", "timestamp"];
const COMMENT_TEXT_FIELDS: &[&str] =
    &["commentcontent", "comment", "content", "text", "commenttext"];
const POST_TEXT_FIELDS: &[&str] = &["title", "caption", "description", "content", "text"];
const LINK_FIELDS: &[&str] = &[
    "originalpostlandingpagelink",
    "videolink",
    "postedvideodownloadlink",
    "downloadlink",
    "link",
    "url",
];
const COMMENT_ID_FIELDS: &[&str] = &["commentid", "commentidstr", "cid"];
const POST_ID_FIELDS: &[&str] =
    &["videoid", "videoidstr", "itemid", "itemidstr", "awemeid", "awemeidstr", "postid"];
const PARENT_VIDEO_ID_FIELDS: &[&str] = &[
    "videoid",
    "videoidstr",
    "itemid",
    "itemidstr",
    "awemeid",
    "awemeidstr",
    "postid",
    "originalpostid",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordKind {
    Comment,
    Post,
}

impl RecordKind {
    fn id_prefix(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Post => "post",
        }
    }

    fn record_type(self) -> RecordType {
        match self {
            Self::Comment => RecordType::Comment,
            Self::Post => RecordType::Post,
        }
    }
}

/// True for the only JSON and TXT entries the TikTok parser understands.
pub fn supported_file(name: &str) -> bool {
    json_file(name) || txt_kind(name).is_some()
}

fn json_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".json") {
        return false;
    }
    if !matches!(
        normalize_key(basename(name)).as_str(),
        "userdatatiktokjson" | "userdatajson"
    ) {
        return false;
    }
    // The export file itself or one ordinary top-level directory prefix.
    // Anything below a private section (messages, profile, and similar) is
    // never a supported export entry.
    let parent = name.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
    let segments: Vec<&str> = parent.split('/').filter(|segment| !segment.is_empty()).collect();
    segments.len() <= 1 && segments.iter().all(|segment| !private_segment(segment))
}

fn private_segment(segment: &str) -> bool {
    matches!(
        normalize_key(segment).as_str(),
        "message"
            | "messages"
            | "dm"
            | "dms"
            | "chat"
            | "chats"
            | "chathistory"
            | "inbox"
            | "directmessage"
            | "directmessages"
            | "private"
            | "profile"
            | "profiles"
            | "profileinformation"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TxtKind {
    Comments,
    Videos,
}

impl TxtKind {
    fn record_kind(self) -> RecordKind {
        match self {
            Self::Comments => RecordKind::Comment,
            Self::Videos => RecordKind::Post,
        }
    }
}

fn txt_kind(name: &str) -> Option<TxtKind> {
    let compact = normalize_key(basename(name));
    let kind = match compact.as_str() {
        "commentstxt" | "commenttxt" => TxtKind::Comments,
        "videostxt" | "videotxt" | "poststxt" | "posttxt" => TxtKind::Videos,
        _ => return None,
    };
    let parent_path = name.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
    let parent_segments: Vec<&str> = parent_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    // Bare file, or one export folder above the known content folder.
    if parent_segments.len() > 2 || parent_segments.iter().any(|segment| private_segment(segment)) {
        return None;
    }
    let parent = normalize_key(basename(parent_path));
    match kind {
        TxtKind::Comments if matches!(parent.as_str(), "" | "comment" | "comments") => Some(kind),
        TxtKind::Videos
            if matches!(parent.as_str(), "" | "video" | "videos" | "post" | "posts") =>
        {
            Some(kind)
        }
        _ => None,
    }
}

fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn invalid_record() -> ImportError {
    ImportErrorCode::InvalidRecord.into()
}

fn invalid_schema() -> ImportError {
    ImportErrorCode::InvalidSchema.into()
}

fn resource_limit() -> ImportError {
    ImportErrorCode::ResourceLimit.into()
}

fn integrity() -> ImportError {
    ImportErrorCode::IntegrityMismatch.into()
}

/// Parses one selected ZIP entry. `file` is the archive-relative name used for
/// `supported_file` and provenance.
pub fn parse_file<R: Read>(
    reader: R,
    file: &str,
    import_id: Uuid,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    if json_file(file) {
        parse_json(reader, file, import_id, emit)
    } else if let Some(kind) = txt_kind(file) {
        parse_txt(reader, file, kind, import_id, emit)
    } else {
        Err(ImportErrorCode::NoSupportedContent.into())
    }
}

fn parse_date(value: &str) -> Result<DateTime<Utc>, ImportError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 64 {
        return Err(invalid_record());
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%d %H:%M:%S %z", "%Y-%m-%d %H:%M:%S%z", "%Y-%m-%dT%H:%M:%S%z"] {
        if let Ok(date) = DateTime::parse_from_str(value, format) {
            return Ok(date.with_timezone(&Utc));
        }
    }
    let without_utc = value.strip_suffix(" UTC").unwrap_or(value);
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if let Ok(date) = NaiveDateTime::parse_from_str(without_utc, format) {
            return Ok(date.and_utc());
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(without_utc, "%Y-%m-%d") {
        if let Some(date) = date.and_hms_opt(0, 0, 0) {
            return Ok(date.and_utc());
        }
    }
    if value.len() <= 19 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(raw) = value.parse::<i64>() {
            let seconds = if raw >= 100_000_000_000 { raw / 1_000 } else { raw };
            if let Some(date) = Utc.timestamp_opt(seconds, 0).single() {
                return Ok(date);
            }
        }
    }
    Err(invalid_record())
}

fn parse_epoch(value: &serde_json::Number) -> Result<DateTime<Utc>, ImportError> {
    let raw = value.as_i64().ok_or_else(invalid_record)?;
    if raw < 0 {
        return Err(invalid_record());
    }
    let seconds = if raw >= 100_000_000_000 { raw / 1_000 } else { raw };
    Utc.timestamp_opt(seconds, 0).single().ok_or_else(invalid_record)
}

fn canonical_date(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn clean_text(value: &str) -> Result<String, ImportError> {
    if value.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(resource_limit());
    }
    let cleaned: String = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect();
    if cleaned.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(resource_limit());
    }
    Ok(cleaned)
}

fn valid_native_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

fn native_id_value(value: &Value) -> Result<Option<String>, ImportError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                Ok(None)
            } else if valid_native_id(text) {
                Ok(Some(text.to_owned()))
            } else {
                Err(invalid_record())
            }
        }
        Value::Number(number) => {
            let text = match number.as_u64() {
                Some(value) => value.to_string(),
                None => match number.as_i64() {
                    Some(value) => value.to_string(),
                    None => return Err(invalid_record()),
                },
            };
            if valid_native_id(&text) {
                Ok(Some(text))
            } else {
                Err(invalid_record())
            }
        }
        _ => Err(invalid_record()),
    }
}

fn find_field<'a>(object: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    for name in names {
        for (key, value) in object {
            if normalize_key(key) == *name {
                return Some(value);
            }
        }
    }
    None
}

// Older exports omit visibility. When present, only explicit public visibility
// qualifies; friends-only, private, and unknown audiences are not public text.
fn public_visibility(value: &str) -> bool {
    matches!(normalize_key(value).as_str(), "everyone" | "everybody" | "public" | "all")
}

fn date_field(object: &Map<String, Value>) -> Result<DateTime<Utc>, ImportError> {
    match find_field(object, DATE_FIELDS) {
        Some(Value::String(value)) => parse_date(value),
        Some(Value::Number(number)) => parse_epoch(number),
        _ => Err(invalid_record()),
    }
}

fn text_field(
    object: &Map<String, Value>,
    names: &[&str],
) -> Result<Option<String>, ImportError> {
    for name in names {
        for (key, value) in object {
            if normalize_key(key) != *name {
                continue;
            }
            match value {
                Value::Null => {}
                Value::String(text) => {
                    if !text.trim().is_empty() {
                        let cleaned = clean_text(text)?;
                        if !cleaned.trim().is_empty() {
                            return Ok(Some(cleaned));
                        }
                    }
                }
                _ => return Err(invalid_record()),
            }
        }
    }
    Ok(None)
}

fn link_field(object: &Map<String, Value>) -> Result<Option<String>, ImportError> {
    for name in LINK_FIELDS {
        for (key, value) in object {
            if normalize_key(key) != *name {
                continue;
            }
            match value {
                Value::Null => {}
                Value::String(text) => {
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    if text.len() > MAX_LINK_BYTES {
                        return Err(resource_limit());
                    }
                    return Ok(Some(text.to_owned()));
                }
                _ => return Err(invalid_record()),
            }
        }
    }
    Ok(None)
}

fn native_id_field(
    object: &Map<String, Value>,
    names: &[&str],
) -> Result<Option<String>, ImportError> {
    for name in names {
        for (key, value) in object {
            if normalize_key(key) != *name {
                continue;
            }
            if let Some(id) = native_id_value(value)? {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// Extracts a numeric video id from a landing page or download URL. The parser
/// never resolves links or downloads content.
fn video_id_from_link(link: &str) -> Option<String> {
    let lower = link.to_ascii_lowercase();
    if let Some(index) = lower.find("/video/") {
        let rest = &link[index + "/video/".len()..];
        let digits: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    if let Some((_, query)) = link.split_once('?') {
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = normalize_key(key);
            if matches!(key.as_str(), "videoid" | "itemid" | "awemeid" | "id") {
                let value = value.split('#').next().unwrap_or(value);
                if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Some(value.to_owned());
                }
            }
        }
    }
    let path = link.split(['?', '#']).next().unwrap_or(link);
    let last = path.rsplit('/').next().unwrap_or("");
    let stem = last.split('.').next().unwrap_or(last);
    if !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(stem.to_owned());
    }
    None
}

fn fallback_id(
    kind: RecordKind,
    source: &str,
    ordinal: u64,
    date: &str,
    text: Option<&str>,
    link: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    for part in ["tiktok", kind.id_prefix(), source, date] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    digest.update(ordinal.to_be_bytes());
    for part in [text.unwrap_or(""), link.unwrap_or("")] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("tiktok:{}:{}", kind.id_prefix(), hex::encode(digest.finalize()))
}

#[allow(clippy::too_many_arguments)]
fn build_record(
    kind: RecordKind,
    created_at: DateTime<Utc>,
    text: Option<String>,
    link: Option<String>,
    native_id: Option<String>,
    parent_video_id: Option<String>,
    parent_confidence: RelationConfidence,
    ordinal: u64,
    import_id: Uuid,
    source: &str,
    family: FormatFamily,
) -> Result<NormalizedArchiveRecord, ImportError> {
    let text = match kind {
        RecordKind::Comment => {
            let text = text.ok_or_else(invalid_record)?;
            if text.trim().is_empty() {
                return Err(invalid_record());
            }
            Some(text)
        }
        RecordKind::Post => text.filter(|value| !value.trim().is_empty()),
    };
    if kind == RecordKind::Post && text.is_none() && link.is_none() {
        return Err(invalid_record());
    }
    let date = canonical_date(created_at);
    let hash = fallback_id(
        kind,
        source,
        ordinal,
        &date,
        text.as_deref(),
        link.as_deref(),
    );
    let logical_id = match kind {
        RecordKind::Comment => match native_id {
            Some(id) => format!("tiktok:comment:{id}"),
            None => hash,
        },
        RecordKind::Post => {
            let video_id = native_id.or_else(|| link.as_deref().and_then(video_id_from_link));
            match video_id {
                Some(id) => format!("tiktok:video:{id}"),
                None => hash,
            }
        }
    };
    let (title, body) = match kind {
        RecordKind::Comment => (None, text),
        RecordKind::Post => match text {
            Some(text) => (Some(text.clone()), Some(text)),
            None => (None, None),
        },
    };
    let (parent_source_id, relation_confidence) = match kind {
        RecordKind::Comment => match parent_video_id {
            Some(id) => (Some(format!("tiktok:video:{id}")), parent_confidence),
            None => (None, RelationConfidence::None),
        },
        RecordKind::Post => (None, RelationConfidence::None),
    };
    Ok(NormalizedArchiveRecord {
        schema_version: 1,
        import_id,
        platform: ArchivePlatform::TikTok,
        source_logical_id: logical_id.clone(),
        source_revision_id: logical_id,
        record_type: kind.record_type(),
        text_format: TextFormat::Standard,
        authorship: Authorship::OwnerAuthored,
        state: ContentState::Active,
        created_at: Some(created_at),
        title,
        body,
        parent_source_id,
        quoted_source_id: None,
        relation_confidence,
        text_truncated: false,
        provenance: RecordProvenance {
            source_file: source.to_owned(),
            source_ordinal: ordinal,
            format_family: family,
            format_confidence: FormatConfidence::Compatible,
        },
    })
}

fn record_from_json(
    value: &Value,
    kind: RecordKind,
    ordinal: u64,
    import_id: Uuid,
    source: &str,
    family: FormatFamily,
) -> Result<Option<NormalizedArchiveRecord>, ImportError> {
    let object = value.as_object().ok_or_else(invalid_record)?;
    if kind == RecordKind::Post {
        if let Some(visibility) = find_field(object, &["whocanview", "whoview", "visibility"]) {
            if !visibility.as_str().is_some_and(public_visibility) { return Ok(None); }
        }
    }
    let text_names = match kind {
        RecordKind::Comment => COMMENT_TEXT_FIELDS,
        RecordKind::Post => POST_TEXT_FIELDS,
    };
    let text = text_field(object, text_names)?;
    // A post without a caption, title, or description is not authored text;
    // the old export often lists only Date/Likes/Link here.
    if kind == RecordKind::Post && text.is_none() {
        return Ok(None);
    }
    let created_at = date_field(object)?;
    let link = link_field(object)?;
    let native_id = match kind {
        RecordKind::Comment => native_id_field(object, COMMENT_ID_FIELDS)?,
        RecordKind::Post => native_id_field(object, POST_ID_FIELDS)?,
    };
    let (parent_video_id, parent_confidence) = match kind {
        RecordKind::Comment => {
            let native_parent = native_id_field(object, PARENT_VIDEO_ID_FIELDS)?;
            if let Some(id) = native_parent {
                (Some(id), RelationConfidence::Explicit)
            } else if let Some(id) = link.as_deref().and_then(video_id_from_link) {
                (Some(id), RelationConfidence::Heuristic)
            } else {
                (None, RelationConfidence::None)
            }
        }
        RecordKind::Post => (None, RelationConfidence::None),
    };
    build_record(
        kind,
        created_at,
        text,
        link,
        native_id,
        parent_video_id,
        parent_confidence,
        ordinal,
        import_id,
        source,
        family,
    )
    .map(Some)
}

fn section_action(kind: RecordKind, name: &str) -> Option<SectionAction> {
    match kind {
        RecordKind::Comment => match name {
            "commentslist" | "commentlist" => Some(SectionAction::List),
            "comments" | "comment" => Some(SectionAction::Nested),
            _ => None,
        },
        RecordKind::Post => match name {
            "videolist" | "postlist" | "postslist" => Some(SectionAction::List),
            "posts" | "videos" | "post" | "video" => Some(SectionAction::Nested),
            _ => None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionAction {
    Nested,
    List,
}

struct JsonState<'a> {
    import_id: Uuid,
    source: &'a str,
    family: FormatFamily,
    emitted: usize,
    emit: &'a mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
}

impl JsonState<'_> {
    fn push(&mut self, value: &Value, kind: RecordKind) -> Result<(), ImportError> {
        if self.emitted >= limits::MAX_NORMALIZED_RECORDS {
            return Err(resource_limit());
        }
        let ordinal = (self.emitted + 1) as u64;
        let Some(record) = record_from_json(
            value,
            kind,
            ordinal,
            self.import_id,
            self.source,
            self.family,
        )? else {
            return Ok(());
        };
        self.emitted += 1;
        (self.emit)(record)
    }
}

fn parse_list<R: Read>(
    cursor: &mut JsonCursor<R>,
    kind: RecordKind,
    depth: usize,
    state: &mut JsonState<'_>,
) -> Result<(), ImportError> {
    cursor.skip_whitespace()?;
    cursor.expect(b'[')?;
    let mut first = true;
    loop {
        cursor.skip_whitespace()?;
        if first && cursor.peek_byte()? == Some(b']') {
            cursor.next_byte()?;
            return Ok(());
        }
        if first && cursor.peek_byte()? == Some(b',') {
            return Err(invalid_schema());
        }
        first = false;
        let element = cursor.read_element_bytes(depth + 1)?;
        let value: Value = serde_json::from_slice(&element).map_err(|_| invalid_schema())?;
        state.push(&value, kind)?;
        cursor.skip_whitespace()?;
        match cursor.next_byte()? {
            Some(b',') => {
                cursor.skip_whitespace()?;
                if cursor.peek_byte()? == Some(b']') {
                    return Err(invalid_schema());
                }
            }
            Some(b']') => return Ok(()),
            _ => return Err(invalid_schema()),
        }
    }
}

fn parse_section_object<R: Read>(
    cursor: &mut JsonCursor<R>,
    kind: RecordKind,
    depth: usize,
    state: &mut JsonState<'_>,
) -> Result<(), ImportError> {
    cursor.skip_whitespace()?;
    cursor.expect(b'{')?;
    let mut first = true;
    loop {
        cursor.skip_whitespace()?;
        if first && cursor.peek_byte()? == Some(b'}') {
            cursor.next_byte()?;
            return Ok(());
        }
        if cursor.peek_byte()? != Some(b'"') {
            return Err(invalid_schema());
        }
        first = false;
        let key = cursor.read_json_string(MAX_JSON_KEY_BYTES)?;
        let name = normalize_key(&key);
        cursor.skip_whitespace()?;
        cursor.expect(b':')?;
        match section_action(kind, &name) {
            Some(SectionAction::Nested) | Some(SectionAction::List) => {
                parse_container(cursor, kind, depth + 1, state)?;
            }
            None => cursor.skip_value(depth + 1)?,
        }
        cursor.skip_whitespace()?;
        match cursor.next_byte()? {
            Some(b',') => {
                cursor.skip_whitespace()?;
                if cursor.peek_byte()? == Some(b'}') {
                    return Err(invalid_schema());
                }
            }
            Some(b'}') => return Ok(()),
            _ => return Err(invalid_schema()),
        }
    }
}

fn parse_container<R: Read>(
    cursor: &mut JsonCursor<R>,
    kind: RecordKind,
    depth: usize,
    state: &mut JsonState<'_>,
) -> Result<(), ImportError> {
    if depth > limits::MAX_JSON_NESTING { return Err(resource_limit()); }
    cursor.skip_whitespace()?;
    match cursor.peek_byte()? {
        Some(b'[') => parse_list(cursor, kind, depth, state),
        Some(b'{') => parse_section_object(cursor, kind, depth, state),
        Some(b'n') => cursor.skip_value(depth),
        Some(b'"') => {
            let value = cursor.read_json_string(limits::MAX_TEXT_FIELD_BYTES)?;
            if value.is_empty() {
                Ok(())
            } else {
                Err(invalid_schema())
            }
        }
        _ => Err(invalid_schema()),
    }
}

fn parse_json<R: Read>(
    reader: R,
    source: &str,
    import_id: Uuid,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    let mut cursor = JsonCursor::new(reader);
    let mut state = JsonState {
        import_id,
        source,
        family: FormatFamily::TikTokJson,
        emitted: 0,
        emit,
    };
    cursor.skip_whitespace()?;
    cursor.expect(b'{')?;
    let mut first = true;
    loop {
        cursor.skip_whitespace()?;
        if first && cursor.peek_byte()? == Some(b'}') {
            cursor.next_byte()?;
            break;
        }
        if cursor.peek_byte()? != Some(b'"') {
            return Err(invalid_schema());
        }
        first = false;
        let key = cursor.read_json_string(MAX_JSON_KEY_BYTES)?;
        let name = normalize_key(&key);
        cursor.skip_whitespace()?;
        cursor.expect(b':')?;
        match name.as_str() {
            "comment" | "comments" => {
                parse_container(&mut cursor, RecordKind::Comment, 2, &mut state)?
            }
            "post" | "posts" | "video" | "videos" => {
                parse_container(&mut cursor, RecordKind::Post, 2, &mut state)?
            }
            "activity" | "youractivity" => {
                parse_activity(&mut cursor, 2, &mut state)?
            }
            _ => cursor.skip_value(2)?,
        }
        cursor.skip_whitespace()?;
        match cursor.next_byte()? {
            Some(b',') => {
                cursor.skip_whitespace()?;
                if cursor.peek_byte()? == Some(b'}') {
                    return Err(invalid_schema());
                }
            }
            Some(b'}') => break,
            _ => return Err(invalid_schema()),
        }
    }
    cursor.skip_whitespace()?;
    if cursor.next_byte()?.is_some() {
        return Err(invalid_schema());
    }
    Ok(state.emitted)
}

/// `Activity` and `Your Activity` hold many private or unrelated lists. Only
/// their comment and post containers are descended into; every other key is
/// skipped without being materialized.
fn parse_activity<R: Read>(
    cursor: &mut JsonCursor<R>,
    depth: usize,
    state: &mut JsonState<'_>,
) -> Result<(), ImportError> {
    if depth > limits::MAX_JSON_NESTING { return Err(resource_limit()); }
    cursor.skip_whitespace()?;
    match cursor.peek_byte()? {
        Some(b'n') => return cursor.skip_value(depth),
        Some(b'{') => {}
        _ => return Err(invalid_schema()),
    }
    cursor.expect(b'{')?;
    let mut first = true;
    loop {
        cursor.skip_whitespace()?;
        if first && cursor.peek_byte()? == Some(b'}') {
            cursor.next_byte()?;
            return Ok(());
        }
        if cursor.peek_byte()? != Some(b'"') {
            return Err(invalid_schema());
        }
        first = false;
        let key = cursor.read_json_string(MAX_JSON_KEY_BYTES)?;
        let name = normalize_key(&key);
        cursor.skip_whitespace()?;
        cursor.expect(b':')?;
        match name.as_str() {
            "comment" | "comments" => {
                parse_container(cursor, RecordKind::Comment, depth + 1, state)?
            }
            "post" | "posts" | "video" | "videos" => {
                parse_container(cursor, RecordKind::Post, depth + 1, state)?
            }
            _ => cursor.skip_value(depth + 1)?,
        }
        cursor.skip_whitespace()?;
        match cursor.next_byte()? {
            Some(b',') => {
                cursor.skip_whitespace()?;
                if cursor.peek_byte()? == Some(b'}') {
                    return Err(invalid_schema());
                }
            }
            Some(b'}') => return Ok(()),
            _ => return Err(invalid_schema()),
        }
    }
}

struct JsonCursor<R> {
    inner: BufReader<R>,
    peeked: Option<u8>,
    read_bytes: u64,
}

impl<R: Read> JsonCursor<R> {
    fn new(reader: R) -> Self {
        Self {
            inner: BufReader::with_capacity(64 * 1024, reader),
            peeked: None,
            read_bytes: 0,
        }
    }

    fn read_byte(&mut self) -> Result<Option<u8>, ImportError> {
        let mut one = [0u8; 1];
        loop {
            match self.inner.read(&mut one) {
                Ok(0) => return Ok(None),
                Ok(_) => {
                    self.read_bytes += 1;
                    if self.read_bytes > limits::MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES {
                        return Err(resource_limit());
                    }
                    return Ok(Some(one[0]));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(integrity()),
            }
        }
    }

    fn next_byte(&mut self) -> Result<Option<u8>, ImportError> {
        match self.peeked.take() {
            Some(byte) => Ok(Some(byte)),
            None => self.read_byte(),
        }
    }

    fn peek_byte(&mut self) -> Result<Option<u8>, ImportError> {
        if self.peeked.is_none() {
            self.peeked = self.read_byte()?;
        }
        Ok(self.peeked)
    }

    fn skip_whitespace(&mut self) -> Result<(), ImportError> {
        while let Some(byte) = self.peek_byte()? {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.next_byte()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn expect(&mut self, expected: u8) -> Result<(), ImportError> {
        match self.next_byte()? {
            Some(byte) if byte == expected => Ok(()),
            _ => Err(invalid_schema()),
        }
    }

    fn read_json_string(&mut self, max_bytes: usize) -> Result<String, ImportError> {
        self.expect(b'"')?;
        let mut raw = Vec::with_capacity(32);
        push_limited(&mut raw, b'"', max_bytes)?;
        let mut escaped = false;
        loop {
            let byte = self.next_byte()?.ok_or_else(invalid_schema)?;
            push_limited(&mut raw, byte, max_bytes)?;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        serde_json::from_slice::<String>(&raw).map_err(|_| invalid_schema())
    }

    fn skip_json_string(&mut self) -> Result<(), ImportError> {
        let mut escaped = false;
        loop {
            let byte = self.next_byte()?.ok_or_else(invalid_schema)?;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return Ok(());
            }
        }
    }

    fn skip_literal(&mut self, suffix: &[u8]) -> Result<(), ImportError> {
        for expected in suffix {
            if self.next_byte()? != Some(*expected) {
                return Err(invalid_schema());
            }
        }
        Ok(())
    }

    fn skip_number(&mut self, first: u8) -> Result<(), ImportError> {
        let mut count = 1usize;
        let mut has_digit = first.is_ascii_digit();
        while let Some(byte) = self.peek_byte()? {
            if matches!(byte, b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E') {
                self.next_byte()?;
                has_digit = has_digit || byte.is_ascii_digit();
                count += 1;
                if count > 64 {
                    return Err(invalid_schema());
                }
            } else {
                break;
            }
        }
        if has_digit {
            Ok(())
        } else {
            Err(invalid_schema())
        }
    }

    fn skip_value(&mut self, depth: usize) -> Result<(), ImportError> {
        if depth > limits::MAX_JSON_NESTING {
            return Err(resource_limit());
        }
        self.skip_whitespace()?;
        let byte = self.next_byte()?.ok_or_else(invalid_schema)?;
        match byte {
            b'"' => self.skip_json_string(),
            b'{' => self.skip_object(depth),
            b'[' => self.skip_array(depth),
            b't' => self.skip_literal(b"rue"),
            b'f' => self.skip_literal(b"alse"),
            b'n' => self.skip_literal(b"ull"),
            b'-' | b'0'..=b'9' => self.skip_number(byte),
            _ => Err(invalid_schema()),
        }
    }

    fn skip_object(&mut self, depth: usize) -> Result<(), ImportError> {
        if depth > limits::MAX_JSON_NESTING {
            return Err(resource_limit());
        }
        self.skip_whitespace()?;
        if self.peek_byte()? == Some(b'}') {
            self.next_byte()?;
            return Ok(());
        }
        loop {
            self.skip_whitespace()?;
            if self.peek_byte()? != Some(b'"') {
                return Err(invalid_schema());
            }
            self.read_json_string(MAX_JSON_KEY_BYTES)?;
            self.skip_whitespace()?;
            self.expect(b':')?;
            self.skip_value(depth + 1)?;
            self.skip_whitespace()?;
            match self.next_byte()? {
                Some(b',') => continue,
                Some(b'}') => return Ok(()),
                _ => return Err(invalid_schema()),
            }
        }
    }

    fn skip_array(&mut self, depth: usize) -> Result<(), ImportError> {
        if depth > limits::MAX_JSON_NESTING {
            return Err(resource_limit());
        }
        self.skip_whitespace()?;
        if self.peek_byte()? == Some(b']') {
            self.next_byte()?;
            return Ok(());
        }
        loop {
            self.skip_value(depth + 1)?;
            self.skip_whitespace()?;
            match self.next_byte()? {
                Some(b',') => continue,
                Some(b']') => return Ok(()),
                _ => return Err(invalid_schema()),
            }
        }
    }

    fn read_element_bytes(&mut self, depth: usize) -> Result<Vec<u8>, ImportError> {
        if depth > limits::MAX_JSON_NESTING {
            return Err(resource_limit());
        }
        self.skip_whitespace()?;
        let first = self.next_byte()?.ok_or_else(invalid_schema)?;
        let mut buffer = Vec::with_capacity(256);
        push_limited(
            &mut buffer,
            first,
            limits::MAX_JSON_ELEMENT_BYTES,
        )?;
        match first {
            b'"' => {
                let mut escaped = false;
                loop {
                    let byte = self.next_byte()?.ok_or_else(invalid_schema)?;
                    push_limited(&mut buffer, byte, limits::MAX_JSON_ELEMENT_BYTES)?;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
            }
            b'{' | b'[' => {
                let mut level = 1usize;
                let mut in_string = false;
                let mut escaped = false;
                while level > 0 {
                    let byte = self.next_byte()?.ok_or_else(invalid_schema)?;
                    push_limited(&mut buffer, byte, limits::MAX_JSON_ELEMENT_BYTES)?;
                    if in_string {
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            in_string = false;
                        }
                    } else {
                        match byte {
                            b'"' => in_string = true,
                            b'{' | b'[' => {
                                level += 1;
                                if depth + level - 1 > limits::MAX_JSON_NESTING {
                                    return Err(resource_limit());
                                }
                            }
                            b'}' | b']' => level -= 1,
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                while let Some(byte) = self.peek_byte()? {
                    if matches!(byte, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                        break;
                    }
                    self.next_byte()?;
                    push_limited(&mut buffer, byte, limits::MAX_JSON_ELEMENT_BYTES)?;
                }
            }
        }
        Ok(buffer)
    }
}

fn push_limited(buffer: &mut Vec<u8>, byte: u8, max: usize) -> Result<(), ImportError> {
    if buffer.len() >= max {
        return Err(resource_limit());
    }
    buffer.push(byte);
    Ok(())
}

struct TxtState<'a> {
    import_id: Uuid,
    source: &'a str,
    family: FormatFamily,
    emitted: usize,
    emit: &'a mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
}

impl TxtState<'_> {
    fn push(&mut self, record: NormalizedArchiveRecord) -> Result<(), ImportError> {
        self.emitted += 1;
        (self.emit)(record)
    }
}

fn is_txt_text_field(name: &str) -> bool {
    matches!(
        name,
        "comment"
            | "commentcontent"
            | "commenttext"
            | "content"
            | "text"
            | "title"
            | "caption"
            | "description"
    )
}

fn is_txt_header(line: &str) -> bool {
    matches!(
        normalize_key(line).as_str(),
        "comment" | "comments" | "post" | "posts" | "video" | "videos"
    )
}

fn is_txt_field(name: &str) -> bool {
    is_txt_text_field(name)
        || matches!(
            name,
            "date"
                | "link"
                | "url"
                | "videolink"
                | "originalpostlandingpagelink"
                | "postedvideodownloadlink"
                | "downloadlink"
                | "commentid"
                | "commentidstr"
                | "cid"
                | "videoid"
                | "videoidstr"
                | "itemid"
                | "itemidstr"
                | "awemeid"
                | "awemeidstr"
                | "postid"
                | "originalpostid"
                | "likes"
                | "receivedlikesnum"
                | "whocanview"
                | "whoview"
                | "visibility"
                | "allowcomments"
                | "allowstitches"
                | "allowduets"
                | "allowstickers"
                | "allowsharingtostory"
                | "contentdisclosure"
                | "aigeneratedcontent"
                | "location"
                | "sound"
                | "addyourtext"
                | "alternatetext"
                | "numberofcollections"
                | "coverimage"
                | "photo"
                | "datedeleted"
                | "from"
        )
}

fn read_txt_line<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    total: &mut u64,
) -> Result<bool, ImportError> {
    line.clear();
    loop {
        let (take, newline) = {
            let buffer = reader.fill_buf().map_err(|_| integrity())?;
            if buffer.is_empty() {
                return Ok(!line.is_empty());
            }
            match buffer.iter().position(|byte| *byte == b'\n') {
                Some(index) => (index + 1, true),
                None => (buffer.len(), false),
            }
        };
        if take > 0 {
            let buffer = reader.fill_buf().map_err(|_| integrity())?;
            line.extend_from_slice(&buffer[..take]);
            reader.consume(take);
            *total = total.saturating_add(take as u64);
            if *total > limits::MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES {
                return Err(resource_limit());
            }
        }
        if line.len() > MAX_TXT_LINE_BYTES {
            return Err(resource_limit());
        }
        if newline {
            line.pop(); // Remove LF before an optional CR.
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(true);
        }
    }
}

fn txt_record_size(fields: &BTreeMap<String, String>) -> Result<(), ImportError> {
    let mut total = 0usize;
    for value in fields.values() {
        total = total.saturating_add(value.len());
        if total > limits::MAX_JSON_ELEMENT_BYTES {
            return Err(resource_limit());
        }
    }
    Ok(())
}

fn set_txt_field(
    fields: &mut BTreeMap<String, String>,
    name: &str,
    value: &str,
    append_text: bool,
) -> Result<(), ImportError> {
    if append_text && is_txt_text_field(name) {
        if let Some(existing) = fields.get_mut(name) {
            existing.push('\n');
            existing.push_str(value);
            if existing.len() > limits::MAX_TEXT_FIELD_BYTES {
                return Err(resource_limit());
            }
        } else {
            fields.insert(name.to_owned(), value.to_owned());
        }
    } else if let Some(existing) = fields.get_mut(name) {
        if existing.is_empty() {
            existing.push_str(value);
        }
    } else {
        fields.insert(name.to_owned(), value.to_owned());
    }
    txt_record_size(fields)
}

fn append_txt_continuation(
    fields: &mut BTreeMap<String, String>,
    name: &str,
    line: &str,
) -> Result<(), ImportError> {
    if let Some(existing) = fields.get_mut(name) {
        existing.push('\n');
        existing.push_str(line);
        if existing.len() > limits::MAX_TEXT_FIELD_BYTES + 65_536 {
            return Err(resource_limit());
        }
    } else {
        fields.insert(name.to_owned(), line.to_owned());
    }
    txt_record_size(fields)
}

fn txt_text(
    fields: &BTreeMap<String, String>,
    kind: RecordKind,
) -> Result<Option<String>, ImportError> {
    let names = match kind {
        RecordKind::Comment => COMMENT_TEXT_FIELDS,
        RecordKind::Post => POST_TEXT_FIELDS,
    };
    for name in names {
        if let Some(value) = fields.get(*name) {
            if value.trim().is_empty() {
                continue;
            }
            let cleaned = clean_text(value)?;
            let cleaned = cleaned.trim_end_matches('\n');
            if !cleaned.trim().is_empty() {
                return Ok(Some(cleaned.to_owned()));
            }
        }
    }
    Ok(None)
}

fn txt_link(fields: &BTreeMap<String, String>) -> Result<Option<String>, ImportError> {
    for name in LINK_FIELDS {
        if let Some(value) = fields.get(*name) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if value.len() > MAX_LINK_BYTES {
                return Err(resource_limit());
            }
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

fn txt_native_id(
    fields: &BTreeMap<String, String>,
    kind: RecordKind,
) -> Result<Option<String>, ImportError> {
    let names = match kind {
        RecordKind::Comment => COMMENT_ID_FIELDS,
        RecordKind::Post => POST_ID_FIELDS,
    };
    for name in names {
        if let Some(value) = fields.get(*name) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if !valid_native_id(value) {
                return Err(invalid_record());
            }
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

fn txt_parent_video_id(
    fields: &BTreeMap<String, String>,
) -> Result<Option<String>, ImportError> {
    for name in PARENT_VIDEO_ID_FIELDS {
        if let Some(value) = fields.get(*name) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if !valid_native_id(value) {
                return Err(invalid_record());
            }
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

fn flush_txt(
    fields: &BTreeMap<String, String>,
    kind: RecordKind,
    state: &mut TxtState<'_>,
) -> Result<(), ImportError> {
    if fields.is_empty() {
        return Ok(());
    }
    if kind == RecordKind::Post && ["whocanview", "whoview", "visibility"].iter()
        .filter_map(|key| fields.get(*key)).any(|value| !public_visibility(value)) {
        return Ok(());
    }
    let text = txt_text(fields, kind)?;
    if kind == RecordKind::Post && text.is_none() {
        return Ok(());
    }
    let date_value = fields.get("date").ok_or_else(invalid_record)?;
    let created_at = parse_date(date_value)?;
    let link = txt_link(fields)?;
    let native_id = txt_native_id(fields, kind)?;
    let (parent_video_id, parent_confidence) = match kind {
        RecordKind::Comment => {
            let native_parent = txt_parent_video_id(fields)?;
            if let Some(id) = native_parent {
                (Some(id), RelationConfidence::Explicit)
            } else if let Some(id) = link.as_deref().and_then(video_id_from_link) {
                (Some(id), RelationConfidence::Heuristic)
            } else {
                (None, RelationConfidence::None)
            }
        }
        RecordKind::Post => (None, RelationConfidence::None),
    };
    if state.emitted >= limits::MAX_NORMALIZED_RECORDS {
        return Err(resource_limit());
    }
    let ordinal = (state.emitted + 1) as u64;
    let record = build_record(
        kind,
        created_at,
        text,
        link,
        native_id,
        parent_video_id,
        parent_confidence,
        ordinal,
        state.import_id,
        state.source,
        state.family,
    )?;
    state.push(record)
}

fn parse_txt<R: Read>(
    reader: R,
    source: &str,
    kind: TxtKind,
    import_id: Uuid,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    let record_kind = kind.record_kind();
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut state = TxtState {
        import_id,
        source,
        family: FormatFamily::TikTokTxt,
        emitted: 0,
        emit,
    };
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    let mut line = Vec::new();
    let mut total = 0u64;
    loop {
        if !read_txt_line(&mut reader, &mut line, &mut total)? {
            break;
        }
        let text = std::str::from_utf8(&line).map_err(|_| ImportErrorCode::InvalidEncoding)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        if fields.is_empty() && last.is_none() && is_txt_header(text) {
            continue;
        }
        if let Some((raw_key, raw_value)) = text.split_once(':') {
            let name = normalize_key(raw_key);
            if is_txt_field(&name) {
                let value = raw_value.strip_prefix(' ').unwrap_or(raw_value);
                if name == "date" {
                    flush_txt(&fields, record_kind, &mut state)?;
                    fields.clear();
                    set_txt_field(&mut fields, "date", value, false)?;
                    last = Some("date".to_owned());
                } else {
                    set_txt_field(&mut fields, &name, value, true)?;
                    last = Some(name);
                }
                continue;
            }
        }
        if text.is_empty() {
            if let Some(name) = last
                .as_deref()
                .filter(|name| is_txt_text_field(name))
            {
                append_txt_continuation(&mut fields, name, "")?;
            }
            continue;
        }
        if let Some(name) = last.as_deref() {
            append_txt_continuation(&mut fields, name, text)?;
        } else if !fields.is_empty() {
            // Once a record has started, an unlabelled non-empty line cannot
            // be attached to any authored field.
            return Err(invalid_record());
        }
        // Before the first recognized field, ignore document titles and other
        // plain-text preamble emitted by some export versions.
    }
    flush_txt(&fields, record_kind, &mut state)?;
    Ok(state.emitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[u8], file: &str) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
        let mut records = Vec::new();
        parse_file(input, file, Uuid::nil(), &mut |record| {
            records.push(record);
            Ok(())
        })?;
        Ok(records)
    }

    #[test]
    fn supported_file_uses_an_explicit_public_allowlist() {
        assert!(supported_file("user_data_tiktok.json"));
        assert!(supported_file("TikTok_Data_2026/user_data.json"));
        assert!(supported_file("Comments/Comments.txt"));
        assert!(supported_file("TikTok_Data_2026/Video/Videos.txt"));
        assert!(supported_file("Posts/Posts.txt"));
        assert!(supported_file("TikTok_Data_2026/Comments/Comments.txt"));
        assert!(!supported_file("Direct Messages.txt"));
        assert!(!supported_file("Profile/Profile Information.txt"));
        assert!(!supported_file("your_instagram_activity/media/posts_1.json"));
        assert!(!supported_file("Comments/Other.txt"));
        assert!(!supported_file("other/user_data_tiktok.json.bak"));
        assert!(!supported_file("messages/inbox/user_data_tiktok.json"));
        assert!(!supported_file("Direct Messages/user_data.json"));
        assert!(!supported_file("inbox/Comments/Comments.txt"));
        assert!(!supported_file("TikTok_Data_2026/messages/Comments/Comments.txt"));
        assert!(!supported_file("TikTok_Data_2026/private/Video/Videos.txt"));
        assert!(!supported_file("wrapper/extra/user_data_tiktok.json"));
        assert!(!supported_file("wrapper/extra/Comments/Comments.txt"));
    }

    #[test]
    fn json_full_carries_comments_and_posts_with_stable_ids() {
        let input = br#"{
            "Profile": {"Profile Information": {"ProfileMap": {"userName": "owner"}}},
            "Comment": {"Comments": {"CommentsList": [
                {"Date": "2024-01-02 03:04:05", "Comment": "first public comment",
                 "Link": "https://www.tiktok.com/@owner/video/7300000000000000001"},
                {"date": "2024-01-03 04:05:06", "comment content": "second public comment",
                 "original post landing page link": "https://www.tiktok.com/@other/video/7300000000000000002"}
            ]}},
            "Post": {"Posts": {"VideoList": [
                {"Date": "2024-02-02 03:04:05", "Title": "public caption",
                 "Who can view": "Everyone", "Likes": 10,
                 "Link": "https://www.tiktok.com/@owner/video/7300000000000000001"}
            ]}},
            "Direct Messages": {"ChatHistory": {"Chat History with friend:": [
                {"Date": "2024-01-04 00:00:00", "From": "friend", "Content": "PRIVATE MUST NOT IMPORT"}
            ]}},
            "Your Activity": {"Like List": {"ItemFavoriteList": [
                {"Date": "2024-01-05 00:00:00", "Link": "https://example.invalid/DO_NOT_IMPORT"}
            ]}}
        }"#;
        let records = parse(input, "user_data_tiktok.json").unwrap();
        assert_eq!(records.len(), 3);
        let comments: Vec<_> = records
            .iter()
            .filter(|record| record.record_type == RecordType::Comment)
            .collect();
        let posts: Vec<_> = records
            .iter()
            .filter(|record| record.record_type == RecordType::Post)
            .collect();
        assert_eq!(comments.len(), 2);
        assert_eq!(posts.len(), 1);
        assert_eq!(comments[0].body.as_deref(), Some("first public comment"));
        assert_eq!(comments[1].body.as_deref(), Some("second public comment"));
        assert_eq!(posts[0].body.as_deref(), Some("public caption"));
        assert_eq!(posts[0].title.as_deref(), Some("public caption"));
        assert_eq!(posts[0].created_at.unwrap().timestamp(), 1_706_843_045);
        assert_eq!(posts[0].source_revision_id, "tiktok:video:7300000000000000001");
        assert_eq!(
            comments[0].parent_source_id.as_deref(),
            Some(posts[0].source_logical_id.as_str())
        );
        for record in &records {
            assert_eq!(record.schema_version, 1);
            assert_eq!(record.platform, ArchivePlatform::TikTok);
            assert_eq!(record.authorship, Authorship::OwnerAuthored);
            assert_eq!(record.provenance.format_family, FormatFamily::TikTokJson);
            assert!(!record.body.as_deref().unwrap_or_default().contains("PRIVATE"));
        }
        let again = parse(input, "user_data_tiktok.json").unwrap();
        assert_eq!(records, again);
    }

    #[test]
    fn json_variant_accepts_singular_wrappers_and_case_variants() {
        let input = br#"{
            "video": {"videos": {"videolist": [
                {"date": "2024-03-01 09:00:00", "title": "variant caption",
                 "link": "https://www.tiktok.com/@owner/video/7300000000000000010"}
            ]}},
            "COMMENT": {"CommentsList": [
                {"DATE": "2024-03-02 09:00:00", "COMMENT": "variant comment",
                 "LINK": "https://www.tiktok.com/@owner/video/7300000000000000010"}
            ]}
        }"#;
        let records = parse(input, "TikTok_Data_2026/user_data_tiktok.json").unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|record| record.record_type == RecordType::Post
            && record.body.as_deref() == Some("variant caption")));
        assert!(records.iter().any(|record| record.record_type == RecordType::Comment
            && record.body.as_deref() == Some("variant comment")));
    }

    #[test]
    fn json_missing_public_sections_yields_no_records() {
        let input = br#"{
            "Direct Messages": {"Direct Messages": {"ChatHistory": {
                "Chat History with friend:": [
                    {"Date": "2024-01-04 00:00:00", "From": "friend", "Content": "PRIVATE"}
                ]
            }}},
            "Comment": {"Comments": {"CommentsList": []}},
            "Post": {"Posts": {"VideoList": []}}
        }"#;
        assert!(parse(input, "user_data.json").unwrap().is_empty());
    }

    #[test]
    fn json_malformed_record_fails_closed() {
        let input = br#"{"Comment": {"Comments": {"CommentsList": [
            {"Comment": "missing date"}
        ]}}}"#;
        assert_eq!(
            parse(input, "user_data_tiktok.json").unwrap_err().code,
            "invalid_record"
        );
    }

    #[test]
    fn json_nesting_element_and_text_limits_are_enforced() {
        let mut deep = String::from("{\"Ignored\": ");
        for _ in 0..(limits::MAX_JSON_NESTING + 2) {
            deep.push('[');
        }
        for _ in 0..(limits::MAX_JSON_NESTING + 2) {
            deep.push(']');
        }
        deep.push('}');
        assert_eq!(
            parse(deep.as_bytes(), "user_data_tiktok.json").unwrap_err().code,
            "resource_limit"
        );

        let oversized = format!(
            "{{\"Comment\":{{\"Comments\":{{\"CommentsList\":[{{\"Date\":\"2024-01-01 00:00:00\",\"Comment\":\"{}\"}}]}}}}}}",
            "a".repeat(limits::MAX_JSON_ELEMENT_BYTES + 1)
        );
        assert_eq!(
            parse(oversized.as_bytes(), "user_data_tiktok.json").unwrap_err().code,
            "resource_limit"
        );

        let long_text = format!(
            "{{\"Comment\":{{\"Comments\":{{\"CommentsList\":[{{\"Date\":\"2024-01-01 00:00:00\",\"Comment\":\"{}\"}}]}}}}}}",
            "a".repeat(limits::MAX_TEXT_FIELD_BYTES + 1)
        );
        assert_eq!(
            parse(long_text.as_bytes(), "user_data_tiktok.json").unwrap_err().code,
            "resource_limit"
        );
    }

    #[test]
    fn txt_comments_preserve_multiline_text_and_crlf() {
        let input = b"Date: 2024-04-01 10:00:00\r\nComment: first line\r\nsecond line\r\n\r\nfourth line\r\nLink: https://www.tiktok.com/@owner/video/7300000000000000011\r\nPhoto: None\r\nUrl: https://www.tiktok.com/@owner/video/7300000000000000011\r\n\r\nDate: 2024-04-02 11:00:00\r\nComment: second comment\r\n";
        let records = parse(input, "Comments/Comments.txt").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].body.as_deref(),
            Some("first line\nsecond line\n\nfourth line")
        );
        assert_eq!(
            records[0].parent_source_id.as_deref(),
            Some("tiktok:video:7300000000000000011")
        );
        assert_eq!(records[1].body.as_deref(), Some("second comment"));
        assert_eq!(records[0].created_at.unwrap().timestamp(), 1_711_965_600);
        assert_eq!(records[0].provenance.format_family, FormatFamily::TikTokTxt);
        assert_eq!(records, parse(input, "Comments/Comments.txt").unwrap());
    }

    #[test]
    fn txt_videos_carry_captions_and_metadata_is_ignored() {
        let input = b"Date: 2024-03-01 09:00:00\nTitle: public video caption\nWho can view: Everyone\nAllow comments: On\nLikes: 12\nLink: https://www.tiktok.com/@owner/video/7300000000000000012\n\nDate: 2024-03-02 09:00:00\nTitle: public post caption\nline two\nLink: https://www.tiktok.com/@owner/video/7300000000000000013\n";
        let records = parse(input, "Video/Videos.txt").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].body.as_deref(), Some("public video caption"));
        assert_eq!(
            records[1].body.as_deref(),
            Some("public post caption\nline two")
        );
        assert_eq!(records[0].source_revision_id, "tiktok:video:7300000000000000012");
    }

    #[test]
    fn txt_malformed_record_fails_closed() {
        assert_eq!(
            parse(b"Comment: missing date\n", "Comments.txt")
                .unwrap_err()
                .code,
            "invalid_record"
        );
        assert_eq!(
            parse(b"Date: not-a-date\nComment: text\n", "Comments/Comments.txt")
                .unwrap_err()
                .code,
            "invalid_record"
        );
        assert!(parse(b"unexpected preamble\n", "Comments/Comments.txt")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn json_activity_paths_read_only_comments_and_posts() {
        let input = br#"{
            "Your Activity": {
                "Comments": {"CommentsList": [
                    {"Date": "2024-05-01 01:02:03", "Comment": "activity comment"}
                ]},
                "Posts": {"PostList": [
                    {"Date": "2024-05-02 01:02:03", "Caption": "activity caption"}
                ]},
                "Direct Messages": {"CommentsList": [
                    {"Date": "2024-05-03 01:02:03", "Comment": "PRIVATE MUST NOT IMPORT"}
                ]}
            },
            "Activity": {
                "Comment": {"Comments": {"CommentsList": [
                    {"Date": "2024-05-04 01:02:03", "Comment": "nested activity comment"}
                ]}},
                "Like List": {"ItemFavoriteList": [
                    {"Date": "2024-05-05 01:02:03", "Link": "https://example.invalid/DO_NOT_IMPORT"}
                ]}
            }
        }"#;
        let records = parse(input, "user_data_tiktok.json").unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .filter(|record| record.record_type == RecordType::Comment)
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.record_type == RecordType::Post)
                .count(),
            1
        );
        assert!(records
            .iter()
            .all(|record| !record.body.as_deref().unwrap_or_default().contains("PRIVATE")));
    }

    #[test]
    fn json_captionless_video_is_skipped() {
        let input = br#"{"Video": {"Videos": {"VideoList": [
            {"Date": "2024-06-01 01:02:03", "Likes": 1,
             "Link": "https://www.tiktok.com/@owner/video/7300000000000000099"}
        ]}}}"#;
        assert!(parse(input, "user_data_tiktok.json").unwrap().is_empty());
    }

    #[test]
    fn txt_metadata_never_leaks_into_authored_text() {
        let comments = parse(
            b"Date: 2024-07-01 01:02:03\nComment: public\nPhoto: example.com/photo.jpg\nmore metadata\nLikes:99\n",
            "Comments/Comments.txt",
        )
        .unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body.as_deref(), Some("public"));

        let posts = parse(
            b"Date: 2024-07-02 01:02:03\nDescription: public\nLikes:99\n",
            "Post/Posts.txt",
        )
        .unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].body.as_deref(), Some("public"));
    }

    #[test]
    fn fixture_trees_parse_with_expected_public_records() {
        fn fixture(relative: &str) -> Vec<u8> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(relative);
            std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
        }

        let json_full = parse(
            &fixture("tiktok/json_full/user_data_tiktok.json"),
            "user_data_tiktok.json",
        )
        .unwrap();
        assert_eq!(json_full.len(), 4);
        assert!(json_full
            .iter()
            .all(|record| record.source_logical_id.starts_with("tiktok:")
                && record.source_revision_id.starts_with("tiktok:")));
        assert_eq!(
            json_full
                .iter()
                .filter(|record| record.record_type == RecordType::Comment)
                .count(),
            2
        );
        assert_eq!(
            json_full
                .iter()
                .filter(|record| record.record_type == RecordType::Post)
                .count(),
            2
        );
        assert!(json_full.iter().all(|record| !record
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("PRIVATE")
            && !record
                .body
                .as_deref()
                .unwrap_or_default()
                .contains("DO_NOT_IMPORT")));

        let json_variant = parse(
            &fixture("tiktok/json_variant/user_data.json"),
            "user_data.json",
        )
        .unwrap();
        assert_eq!(json_variant.len(), 2);
        assert!(json_variant.iter().any(|record| record.record_type == RecordType::Comment));
        assert!(json_variant.iter().any(|record| record.record_type == RecordType::Post));

        let comments = parse(
            &fixture("tiktok/txt_full/Comments/Comments.txt"),
            "Comments/Comments.txt",
        )
        .unwrap();
        assert_eq!(comments.len(), 2);
        let posts = parse(
            &fixture("tiktok/txt_full/Video/Videos.txt"),
            "Video/Videos.txt",
        )
        .unwrap();
        assert_eq!(posts.len(), 1);

        let variant_comments = parse(
            &fixture("tiktok/txt_variant/Comments/Comments.txt"),
            "Comments/Comments.txt",
        )
        .unwrap();
        assert_eq!(variant_comments.len(), 2);
        let variant_posts = parse(
            &fixture("tiktok/txt_variant/Post/Posts.txt"),
            "Post/Posts.txt",
        )
        .unwrap();
        assert_eq!(variant_posts.len(), 1);

        for root in ["tiktok/json_missing", "tiktok/json_dm_ignored"] {
            assert!(parse(
                &fixture(&format!("{root}/user_data_tiktok.json")),
                "user_data_tiktok.json"
            )
            .unwrap()
            .is_empty());
        }

        assert_eq!(
            parse(
                &fixture("tiktok/json_malformed/user_data_tiktok.json"),
                "user_data_tiktok.json"
            )
            .unwrap_err()
            .code,
            "invalid_record"
        );
        assert_eq!(
            parse(
                &fixture("tiktok/txt_malformed/Comments/Comments.txt"),
                "Comments/Comments.txt"
            )
            .unwrap_err()
            .code,
            "invalid_record"
        );
    }

    #[test]
    fn known_wrapper_recursion_obeys_the_nesting_cap() {
        let mut input = String::from(r#"{"CommentsList":[]}"#);
        for _ in 0..limits::MAX_JSON_NESTING {
            input = format!(r#"{{"Comments":{input}}}"#);
        }
        let document = format!(r#"{{"Comment":{input}}}"#);
        assert_eq!(parse(document.as_bytes(), "user_data.json").unwrap_err().code, "resource_limit");
    }

    #[test]
    fn activity_video_wrappers_include_authored_captions() {
        for activity in ["Activity", "Your Activity"] {
            let document = format!(r#"{{"{activity}":{{"Video":{{"Videos":{{"VideoList":[{{"Date":"2026-01-01 00:00:00","Description":"Public video caption"}}]}}}}}}}}"#);
            let rows = parse(document.as_bytes(), "user_data.json").unwrap();
            assert_eq!(rows[0].body.as_deref(), Some("Public video caption"));
            assert_eq!(rows[0].record_type, RecordType::Post);
        }
    }

    #[test]
    fn unsupported_file_is_rejected() {
        assert_eq!(
            parse(b"private", "messages/inbox/private.json")
                .unwrap_err()
                .code,
            "no_supported_content"
        );
    }
}
