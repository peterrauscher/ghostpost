//! Instagram "Download Your Information" JSON parsing.
//!
//! Only owner-authored posts, stories, reels, archived posts, and comments are
//! normalized. DMs, likes, saved items, followers, profile photos, and every
//! other engagement file are deliberately unsupported.
//!
//! JSON is the only accepted export variant. Meta's HTML export wraps the same
//! metadata in obfuscated, version-unstable table markup with no machine
//! description; it is rejected (an HTML-only archive selects no entries and
//! surfaces `no_supported_content`) rather than scraped heuristically.
//!
//! Recognized layouts, including the `content/` and root-level `comments/`
//! paths used by older exports:
//! - `your_instagram_activity/media/posts_N.json` (`content/posts_N.json`)
//! - `your_instagram_activity/media/archived_posts.json` (`content/...`)
//! - `your_instagram_activity/media/stories.json` (`content/stories.json`)
//! - `your_instagram_activity/media/reels.json` (`content/reels.json`)
//! - `your_instagram_activity/comments/post_comments_N.json`
//!   (`comments/post_comments_N.json`)
//! - `your_instagram_activity/comments/reels_comments.json`
//! - `comments/comments.json` / `comments/post_comments.json` with the legacy
//!   `comments_media_comments` + `string_list_data` shape
//!
//! The archive root folder (`instagram-{user}-{date}-{token}/`) is variable, so
//! `supported_file` accepts these paths after a root prefix, at path-segment
//! boundaries only. It never accepts a bare filename or a path nested under
//! `messages/`.
use super::json;
use super::*;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::Read;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Posts,
    Stories,
    Reels,
    ArchivedPosts,
    Comments,
}

fn numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|number| {
            !number.is_empty()
                && number.len() <= 4
                && number.bytes().all(|byte| byte.is_ascii_digit())
                && number.parse::<u32>().is_ok_and(|number| number > 0)
        })
}

fn classify(name: &str) -> Option<Kind> {
    match name {
        "your_instagram_activity/media/stories.json"
        | "your_instagram_activity/content/stories.json"
        | "content/stories.json" => Some(Kind::Stories),
        "your_instagram_activity/media/reels.json" | "content/reels.json" => Some(Kind::Reels),
        "your_instagram_activity/media/archived_posts.json" | "content/archived_posts.json" => {
            Some(Kind::ArchivedPosts)
        }
        "your_instagram_activity/comments/post_comments.json"
        | "comments/post_comments.json"
        | "your_instagram_activity/comments/comments.json"
        | "comments/comments.json"
        | "your_instagram_activity/comments/reels_comments.json"
        | "comments/reels_comments.json" => Some(Kind::Comments),
        _ => {
            if numbered(name, "your_instagram_activity/media/posts_") || numbered(name, "content/posts_") {
                Some(Kind::Posts)
            } else if numbered(name, "your_instagram_activity/comments/post_comments_")
                || numbered(name, "comments/post_comments_")
            {
                Some(Kind::Comments)
            } else {
                None
            }
        }
    }
}

/// Locates a recognized file and returns its canonical (root-free) name. Any
/// leading archive-root segments are removed, but a path that descends through
/// `messages/` or `message_requests/` is never treated as activity metadata.
fn locate(name: &str) -> Option<(&str, Kind)> {
    if name.is_empty() || name.starts_with('/') {
        return None;
    }
    let mut rest = name;
    loop {
        if let Some(kind) = classify(rest) {
            return Some((rest, kind));
        }
        let (head, tail) = rest.split_once('/')?;
        if head.is_empty() || matches!(head, "messages" | "message_requests") {
            return None;
        }
        rest = tail;
    }
}

/// Whether this archive entry is one of the Instagram activity files this
/// parser understands.
pub fn supported_file(name: &str) -> bool {
    locate(name).is_some()
}

fn clean_text(value: &str) -> Result<String, ImportError> {
    if value.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    let repaired = repair_latin1(value);
    let normalized: String = repaired
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect();
    if normalized.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    Ok(normalized)
}

/// Meta sometimes emits UTF-8 bytes re-decoded as ISO-8859-1. Repair only when
/// every character fits a Latin-1 byte and the resulting bytes are valid UTF-8;
/// genuine Latin-1 text (whose bytes are not valid UTF-8) and already-correct
/// Unicode are left untouched. ASCII captions, by far the common case, borrow.
fn repair_latin1(value: &str) -> Cow<'_, str> {
    if value.is_ascii() {
        return Cow::Borrowed(value);
    }
    let mut bytes = Vec::with_capacity(value.len());
    for character in value.chars() {
        let code = character as u32;
        if code > 0xFF {
            return Cow::Borrowed(value);
        }
        bytes.push(code as u8);
    }
    match std::str::from_utf8(&bytes) {
        Ok(repaired) if repaired != value => Cow::Owned(repaired.to_owned()),
        _ => Cow::Borrowed(value),
    }
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ImportError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ImportErrorCode::InvalidRecord.into()),
    }
}

fn timestamp_field(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<DateTime<Utc>>, ImportError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let seconds = number
                .as_i64()
                .filter(|seconds| *seconds >= 0)
                .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
            DateTime::from_timestamp(seconds, 0)
                .map(Some)
                .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))
        }
        Some(_) => Err(ImportErrorCode::InvalidRecord.into()),
    }
}

fn wrapper_allowed(kind: Kind, wrapper: Option<&str>) -> bool {
    match kind {
        Kind::Posts | Kind::ArchivedPosts => wrapper.is_none(),
        Kind::Stories => matches!(wrapper, None | Some("ig_stories")),
        Kind::Reels => matches!(wrapper, None | Some("ig_reels_media")),
        Kind::Comments => matches!(
            wrapper,
            None | Some("comments_media_comments") | Some("comments_reels_comments")
        ),
    }
}

fn record_id(source: &str, ordinal: u64) -> String {
    format!("instagram:{source}:{ordinal}")
}

fn build_record(
    import_id: Uuid,
    source: &str,
    ordinal: u64,
    record_type: RecordType,
    created_at: DateTime<Utc>,
    body: Option<String>,
) -> NormalizedArchiveRecord {
    let id = record_id(source, ordinal);
    NormalizedArchiveRecord {
        schema_version: 1,
        import_id,
        platform: ArchivePlatform::Instagram,
        source_logical_id: id.clone(),
        source_revision_id: id,
        record_type,
        text_format: TextFormat::Standard,
        authorship: Authorship::OwnerAuthored,
        state: ContentState::Active,
        created_at: Some(created_at),
        title: None,
        body,
        parent_source_id: None,
        quoted_source_id: None,
        relation_confidence: RelationConfidence::None,
        text_truncated: false,
        provenance: RecordProvenance {
            source_file: source.to_owned(),
            source_ordinal: ordinal,
            format_family: FormatFamily::InstagramJson,
            format_confidence: FormatConfidence::Compatible,
        },
    }
}

fn push_record(
    record: NormalizedArchiveRecord,
    emitted: &mut usize,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    if *emitted >= limits::MAX_NORMALIZED_RECORDS {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    emit(record)?;
    *emitted += 1;
    Ok(())
}

struct MediaInfo {
    title: Option<String>,
    timestamp: Option<DateTime<Utc>>,
}

/// Handles `posts_N.json`, `archived_posts.json`, and one `ig_reels_media`
/// group. One normalized Post is emitted per export entry. The non-empty
/// post-level title is the caption and takes precedence. When it is absent,
/// every distinct non-empty media title is kept, in media order, joined with
/// newlines (single-image exports often keep the caption at
/// `media[0].title`, and carousels may carry per-frame text). Textless entries
/// are skipped. Each record needs a timestamp, falling back from the
/// post-level date to the earliest media date.
fn parse_post_like(
    value: &Value,
    ordinal: u64,
    import_id: Uuid,
    source: &str,
    emitted: &mut usize,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let object = value
        .as_object()
        .filter(|object| !object.is_empty())
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let outer_title = string_field(object, "title")?
        .map(clean_text)
        .transpose()?
        .filter(|title| !title.trim().is_empty());
    let outer_time = timestamp_field(object, "creation_timestamp")?;
    let media = match object.get("media") {
        None => &[][..],
        Some(Value::Array(media)) => media.as_slice(),
        Some(_) => return Err(ImportErrorCode::InvalidRecord.into()),
    };
    if media.len() > limits::MAX_NORMALIZED_RECORDS {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    let mut infos = Vec::with_capacity(media.len());
    for item in media {
        let item = item
            .as_object()
            .filter(|item| !item.is_empty())
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        let title = string_field(item, "title")?
            .map(clean_text)
            .transpose()?
            .filter(|title| !title.trim().is_empty());
        infos.push(MediaInfo {
            title,
            timestamp: timestamp_field(item, "creation_timestamp")?,
        });
    }
    let earliest_media_time = infos.iter().filter_map(|info| info.timestamp).min();
    let body = match outer_title {
        Some(title) => title,
        None => {
            let mut seen = BTreeSet::new();
            let mut captions = Vec::new();
            for info in infos {
                let Some(title) = info.title else {
                    continue;
                };
                if seen.insert(title.clone()) {
                    captions.push(title);
                }
            }
            if captions.is_empty() {
                return Ok(());
            }
            captions.join("\n")
        }
    };
    if body.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    let created_at = outer_time
        .or(earliest_media_time)
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    push_record(
        build_record(import_id, source, ordinal, RecordType::Post, created_at, Some(body)),
        emitted,
        emit,
    )
}

/// Stories are one media item per `ig_stories` entry. They usually carry no
/// caption, so textless stories are skipped; a story title is normalized only
/// when the export actually contains user text for it.
fn parse_story(
    value: &Value,
    ordinal: u64,
    import_id: Uuid,
    source: &str,
    emitted: &mut usize,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let object = value
        .as_object()
        .filter(|object| !object.is_empty())
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let Some(title) = string_field(object, "title")?
        .map(clean_text)
        .transpose()?
        .filter(|title| !title.trim().is_empty()) else {
        return Ok(());
    };
    let created_at = timestamp_field(object, "creation_timestamp")?
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    push_record(
        build_record(import_id, source, ordinal, RecordType::Post, created_at, Some(title)),
        emitted,
        emit,
    )
}

/// Comments carry no parent media identifier in the export (only the media
/// owner's handle), so no parent relation is invented. Current rows use
/// `string_map_data`; legacy rows use the first `string_list_data` entry.
fn parse_comment(
    value: &Value,
    ordinal: u64,
    import_id: Uuid,
    source: &str,
    emitted: &mut usize,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let object = value
        .as_object()
        .filter(|object| !object.is_empty())
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let (raw_text, created_at) = if let Some(map) = object.get("string_map_data") {
        let map = map
            .as_object()
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        // Only the explicit `Comment` label is text. Labels such as
        // `Comment Author` or `Media Owner` must never be imported as prose.
        let entry = map
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("comment"))
            .map(|(_, value)| value)
            .and_then(Value::as_object)
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        let text = string_field(entry, "value")?
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        let time = map
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("time"))
            .map(|(_, value)| value)
            .and_then(Value::as_object);
        let timestamp = match time {
            Some(time) => timestamp_field(time, "timestamp")?,
            None => None,
        };
        (text, timestamp)
    } else if let Some(list) = object.get("string_list_data") {
        let entry = list
            .as_array()
            .and_then(|list| list.first())
            .and_then(Value::as_object)
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        let text = string_field(entry, "value")?
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        (text, timestamp_field(entry, "timestamp")?)
    } else {
        return Err(ImportErrorCode::InvalidRecord.into());
    };
    let text = clean_text(raw_text)?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let created_at =
        created_at.ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    push_record(
        build_record(
            import_id,
            source,
            ordinal,
            RecordType::Comment,
            created_at,
            Some(text),
        ),
        emitted,
        emit,
    )
}

/// Parses one selected Instagram entry, emitting normalized records. Returns
/// the number of records emitted; an unrecognized filename emits none.
pub fn parse_file<R: Read>(
    reader: R,
    file: &str,
    import_id: Uuid,
    emit: &mut dyn FnMut(NormalizedArchiveRecord) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    let Some((source, kind)) = locate(file) else {
        return Ok(0);
    };
    let mut emitted = 0usize;
    let mut ordinal = 0u64;
    let mut checked_wrapper = false;
    json::stream_array(reader, |wrapper, raw| {
        if !checked_wrapper {
            if !wrapper_allowed(kind, wrapper) {
                return Err(ImportErrorCode::UnsupportedFormatVersion.into());
            }
            checked_wrapper = true;
        }
        ordinal += 1;
        if ordinal as usize > limits::MAX_NORMALIZED_RECORDS {
            return Err(ImportErrorCode::ResourceLimit.into());
        }
        let value = json::parse_element(raw)?;
        match kind {
            Kind::Posts | Kind::Reels | Kind::ArchivedPosts => {
                parse_post_like(&value, ordinal, import_id, source, &mut emitted, emit)
            }
            Kind::Stories => parse_story(&value, ordinal, import_id, source, &mut emitted, emit),
            Kind::Comments => parse_comment(&value, ordinal, import_id, source, &mut emitted, emit),
        }
    })?;
    Ok(emitted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn parse(bytes: &[u8], file: &str) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
        let mut records = Vec::new();
        let emitted = parse_file(Cursor::new(bytes.to_vec()), file, Uuid::nil(), &mut |record| {
            records.push(record);
            Ok(())
        })?;
        assert_eq!(emitted, records.len());
        Ok(records)
    }

    fn error_code(result: Result<Vec<NormalizedArchiveRecord>, ImportError>) -> &'static str {
        result.expect_err("expected a parse error").code
    }

    fn fixture(relative: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/instagram")
            .join(relative);
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    fn body(record: &NormalizedArchiveRecord) -> &str {
        record.body.as_deref().unwrap_or("")
    }

    #[test]
    fn recognizes_root_prefixed_activity_paths_only() {
        assert!(supported_file("your_instagram_activity/media/posts_1.json"));
        assert!(supported_file("instagram-jane-20260101-abc123/your_instagram_activity/media/posts_1.json"));
        assert!(supported_file("wrapped/instagram-jane-20260101-abc123/content/posts_12.json"));
        assert!(supported_file("your_instagram_activity/content/stories.json"));
        assert!(supported_file("content/reels.json"));
        assert!(supported_file("comments/comments.json"));
        assert!(!supported_file("content/posts_0.json"));
        assert!(!supported_file("messages/inbox/friend_1/your_instagram_activity/media/posts_1.json"));
        assert!(!supported_file("messages/inbox/friend_1/posts_1.json"));
        assert!(!supported_file("your_instagram_activity/likes/liked_posts.json"));
        assert!(!supported_file("your_instagram_activity/saved/saved_posts.json"));
        assert!(!supported_file("connections/followers_and_following/followers_1.json"));
        assert!(!supported_file("your_instagram_activity/media/profile_photos.json"));
        assert!(!supported_file("your_instagram_activity/media/igtv_videos.json"));
        assert!(!supported_file("your_instagram_activity/media/posts_1.html"));
    }

    #[test]
    fn emits_one_post_per_entry_with_joined_media_captions() {
        let document = br#"[
          {"title":"Outer caption","creation_timestamp":10,"media":[{"uri":"a","title":""},{"uri":"b","title":""}]},
          {"title":"Outer caption","creation_timestamp":20,"media":[{"uri":"a","title":"Outer caption"},{"uri":"b","title":"Inner caption"}]},
          {"title":"","creation_timestamp":30,"media":[{"uri":"a","title":"First frame"},{"uri":"b","title":"First frame"},{"uri":"c","title":"Third frame"}]},
          {"title":"","creation_timestamp":40,"media":[{"uri":"a","title":""}]}
        ]"#;
        let records = parse(document, "content/posts_1.json").unwrap();
        let bodies: Vec<&str> = records.iter().map(body).collect();
        assert_eq!(
            bodies,
            vec![
                "Outer caption",
                "Outer caption",
                "First frame\nThird frame"
            ]
        );
        let ids: Vec<&str> = records.iter().map(|record| record.source_logical_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "instagram:content/posts_1.json:1",
                "instagram:content/posts_1.json:2",
                "instagram:content/posts_1.json:3",
            ]
        );
        assert_eq!(records[2].created_at, DateTime::from_timestamp(30, 0));
        assert!(records.iter().all(|record| record.created_at.is_some()));
        assert!(records.iter().all(|record| record.title.is_none()));
        assert!(records.iter().all(|record| record.record_type == RecordType::Post));
        assert!(records.iter().all(|record| record.platform == ArchivePlatform::Instagram));
    }

    #[test]
    fn repairs_latin1_mojibake_and_normalizes_nfc() {
        let records = parse(
            br#"[
              {"title":"Caf\u00c3\u00a9","creation_timestamp":1,"media":[{"uri":"a"}]},
              {"title":"Cafe\u0301","creation_timestamp":2,"media":[{"uri":"b"}]},
              {"title":"caf\u00e9","creation_timestamp":3,"media":[{"uri":"c"}]},
              {"title":"\u00f0\u009f\u0098\u0080 caf\u00c3\u00a9","creation_timestamp":4,"media":[{"uri":"d"}]}
            ]"#,
            "your_instagram_activity/media/posts_1.json",
        )
        .unwrap();
        assert_eq!(body(&records[0]), "Café");
        assert_eq!(body(&records[1]), "Café");
        assert_eq!(body(&records[2]), "café");
        assert_eq!(body(&records[3]), "\u{1f600} café");
    }

    #[test]
    fn parses_story_and_reel_wrappers_and_skips_textless_media() {
        let stories = parse(
            br#"{"ig_stories":[{"uri":"media/stories/a.jpg","creation_timestamp":7},{"uri":"media/stories/b.mp4","creation_timestamp":8,"title":"Story text"}]}"#,
            "instagram-jane-20260101-abc/your_instagram_activity/media/stories.json",
        )
        .unwrap();
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].body.as_deref(), Some("Story text"));
        assert!(stories.iter().all(|record| record.body.is_some()));
        assert!(stories.iter().all(|record| record.created_at.is_some()));
        assert!(stories.iter().all(|record| record.record_type == RecordType::Post));

        let reels = parse(
            br#"{"ig_reels_media":[{"media":[{"uri":"media/reels/a.mp4","creation_timestamp":9,"title":"Reel caption"}]},{"media":[{"uri":"media/reels/b.mp4","creation_timestamp":10}]}]}"#,
            "your_instagram_activity/media/reels.json",
        )
        .unwrap();
        assert_eq!(reels.len(), 1);
        assert_eq!(body(&reels[0]), "Reel caption");
        assert!(reels.iter().all(|record| record.created_at.is_some()));

        assert_eq!(
            error_code(parse(
                br#"{"unknown_wrapper":[{"uri":"a"}]}"#,
                "your_instagram_activity/media/stories.json"
            )),
            "unsupported_format_version"
        );
        assert_eq!(
            error_code(parse(
                br#"{"posts":[{"title":"x"}]}"#,
                "content/posts_1.json"
            )),
            "unsupported_format_version"
        );
    }

    #[test]
    fn parses_current_and_legacy_comment_shapes() {
        let current = parse(
            br#"[{"media_owner":"jane_doe","string_map_data":{"Comment":{"value":"Nice light"},"Time":{"value":"","timestamp":1767600000},"Media Owner":{"value":"jane_doe","timestamp":0}}}]"#,
            "your_instagram_activity/comments/post_comments_1.json",
        )
        .unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(body(&current[0]), "Nice light");
        assert_eq!(current[0].record_type, RecordType::Comment);
        assert_eq!(current[0].created_at, DateTime::from_timestamp(1_767_600_000, 0));
        assert_eq!(current[0].parent_source_id, None);

        let legacy = parse(
            br#"{"comments_media_comments":[{"title":"jane_doe","string_list_data":[{"value":"Legacy comment","timestamp":1577923200}],"media_list_data":[]}]}"#,
            "instagram-jane-20200101-abc/comments/comments.json",
        )
        .unwrap();
        assert_eq!(legacy.len(), 1);
        assert_eq!(body(&legacy[0]), "Legacy comment");
        assert_eq!(legacy[0].created_at, DateTime::from_timestamp(1_577_923_200, 0));
        assert_eq!(
            legacy[0].source_logical_id,
            "instagram:comments/comments.json:1"
        );

        let reels = parse(
            br#"{"comments_reels_comments":[{"string_map_data":{"Comment":{"value":"Reel comment"},"Time":{"timestamp":1767600001}}}]}"#,
            "your_instagram_activity/comments/reels_comments.json",
        )
        .unwrap();
        assert_eq!(body(&reels[0]), "Reel comment");

        let blank = parse(
            br#"[{"string_map_data":{"Comment":{"value":"   "},"Time":{"timestamp":1}}}]"#,
            "your_instagram_activity/comments/post_comments_1.json",
        )
        .unwrap();
        assert!(blank.is_empty());
        assert_eq!(
            error_code(parse(
                br#"[{"string_map_data":{"Comment Author":{"value":"jane_doe"},"Time":{"timestamp":1}}}]"#,
                "your_instagram_activity/comments/post_comments_1.json"
            )),
            "invalid_record"
        );
    }

    #[test]
    fn rejects_malformed_records_and_exceeded_limits() {
        assert_eq!(
            error_code(parse(br#"[{}]"#, "content/posts_1.json")),
            "invalid_record"
        );
        assert_eq!(
            error_code(parse(
                br#"[{"string_list_data":[]}]"#,
                "comments/comments.json"
            )),
            "invalid_record"
        );
        let huge = "a".repeat(limits::MAX_TEXT_FIELD_BYTES + 1);
        let oversized = format!(r#"[{{"title":"{huge}","creation_timestamp":1,"media":[{{"uri":"a"}}]}}]"#);
        assert_eq!(
            error_code(parse(oversized.as_bytes(), "content/posts_1.json")),
            "resource_limit"
        );
        let repeated = "a".repeat(limits::MAX_TEXT_FIELD_BYTES / 2 + 1);
        let joined = format!(
            r#"[{{"title":"","creation_timestamp":1,"media":[{{"title":"{repeated}"}},{{"title":"{repeated}b"}}]}}]"#
        );
        assert_eq!(
            error_code(parse(joined.as_bytes(), "content/posts_1.json")),
            "resource_limit"
        );
        let nested = format!(
            "{}0{}",
            "[".repeat(limits::MAX_JSON_NESTING + 1),
            "]".repeat(limits::MAX_JSON_NESTING + 1)
        );
        let deep = format!(r#"[{{"title":"x","creation_timestamp":1,"extra":{nested}}}]"#);
        assert_eq!(
            error_code(parse(deep.as_bytes(), "content/posts_1.json")),
            "resource_limit"
        );
        let malformed = br#"[{"title":"unterminated"#;
        assert_eq!(error_code(parse(malformed, "content/posts_1.json")), "invalid_schema");
        assert_eq!(
            error_code(parse(br#"[{"title":"No timestamp"}]"#, "content/posts_1.json")),
            "invalid_record"
        );
    }

    #[test]
    fn parses_realistic_synthetic_fixtures_without_importing_interactions() {
        let current_root = "synthetic_current_v1/instagram-synthetic-20260101-000000";
        let current_files = [
            "your_instagram_activity/media/posts_1.json",
            "your_instagram_activity/media/posts_2.json",
            "your_instagram_activity/media/stories.json",
            "your_instagram_activity/media/reels.json",
            "your_instagram_activity/media/archived_posts.json",
            "your_instagram_activity/comments/post_comments_1.json",
            "your_instagram_activity/comments/post_comments_2.json",
            "your_instagram_activity/comments/reels_comments.json",
        ];
        let mut current = Vec::new();
        for file in current_files {
            let path = format!("{current_root}/{file}");
            let mut records = parse(&fixture(&path), &path).unwrap();
            current.append(&mut records);
        }
        assert_eq!(current.len(), 12);
        assert_eq!(
            current.iter().filter(|record| record.record_type == RecordType::Comment).count(),
            4
        );
        let bodies: Vec<&str> = current.iter().map(body).collect();
        for expected in [
            "Snow on the ridge #pdx",
            "Two frames, one caption",
            "First frame caption\nThird frame caption",
            "Story text overlay",
            "Café by the lake",
            "Second shard post",
            "Reel caption with café",
            "Archived caption",
            "Lovely light!",
            "Café comments work",
        ] {
            assert!(bodies.contains(&expected), "missing {expected:?} in {bodies:?}");
        }
        assert!(current.iter().all(|record| record.body.is_some()));
        assert!(current.iter().all(|record| record.created_at.is_some()));
        assert!(current.iter().all(|record| record.source_logical_id.starts_with("instagram:")));
        assert!(current
            .iter()
            .all(|record| !record.source_logical_id.contains("instagram-synthetic")));
        assert!(current
            .iter()
            .all(|record| !body(record).contains("DO_NOT_IMPORT")));
        let mut ids = BTreeSet::new();
        assert!(current.iter().all(|record| ids.insert(record.source_logical_id.clone())));

        let legacy_root = "synthetic_legacy_v1/instagram-synthetic-legacy-20200101-000000";
        let legacy_files = [
            "content/posts_1.json",
            "content/stories.json",
            "content/archived_posts.json",
            "comments/comments.json",
            "comments/post_comments_1.json",
        ];
        let mut legacy = Vec::new();
        for file in legacy_files {
            let path = format!("{legacy_root}/{file}");
            let mut records = parse(&fixture(&path), &path).unwrap();
            legacy.append(&mut records);
        }
        assert_eq!(legacy.len(), 8);
        let bodies: Vec<&str> = legacy.iter().map(body).collect();
        for expected in [
            "Legacy caption",
            "Legacy media caption",
            "Legacy Café",
            "Legacy story text",
            "Legacy archived caption",
            "Legacy comment text",
            "Another legacy comment",
        ] {
            assert!(bodies.contains(&expected), "missing {expected:?} in {bodies:?}");
        }
        assert!(legacy.iter().all(|record| record.body.is_some()));
        assert!(legacy.iter().all(|record| record.created_at.is_some()));
        assert!(legacy
            .iter()
            .all(|record| !body(record).contains("DO_NOT_IMPORT")));
    }
}
