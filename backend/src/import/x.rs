//! X archive parsing. `.js` files are streamed one top-level array element at
//! a time (`read_js_array`); cross-record rules (note joins, duplicate
//! revisions, edit chains) are resolved by `XResolver` over `BoundedIndex`es
//! that spill to Postgres, so memory does not grow with the archive.
use super::staging::{BoundedIndex, IndexCursor, SpillTarget};
use super::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

/// Dataset named by a `window.YTD.<dataset>.partN = ` wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    Tweets,
    DeletedTweets,
    NoteTweet,
}

fn byte<R: Read>(reader: &mut R) -> Result<Option<u8>, ImportError> {
    let mut one = [0u8; 1];
    loop {
        match reader.read(&mut one) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(one[0])),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ImportErrorCode::IntegrityMismatch.into()),
        }
    }
}

/// Streams a `window.YTD.<dataset>.partN = [ ... ]` file, calling `element`
/// with the raw bytes of each top-level array element. At most one element
/// (`MAX_JSON_ELEMENT_BYTES`) is buffered. Enforces the wrapper allowlist,
/// `MAX_JSON_NESTING` (the outer array counts as one level), and that only
/// whitespace and an optional `;` follow the array.
pub fn read_js_array<R: Read>(
    reader: R,
    mut element: impl FnMut(Dataset, &[u8]) -> Result<(), ImportError>,
) -> Result<Dataset, ImportError> {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut prefix = Vec::with_capacity(128);
    loop {
        match byte(&mut reader)? {
            Some(b'=') => break,
            Some(other) if prefix.len() < limits::MAX_JS_PREFIX_BYTES => prefix.push(other),
            _ => return Err(ImportErrorCode::UnsupportedFormatVersion.into()),
        }
    }
    let lhs = std::str::from_utf8(&prefix)
        .map_err(|_| ImportError::from(ImportErrorCode::InvalidEncoding))?.trim();
    let Some(name) = lhs.strip_prefix("window.YTD.") else {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    };
    let Some((dataset, part)) = name.rsplit_once(".part") else {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    };
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    let dataset = match dataset {
        "tweets" | "tweet" => Dataset::Tweets,
        "deleted_tweets" => Dataset::DeletedTweets,
        "note_tweet" => Dataset::NoteTweet,
        _ => return Err(ImportErrorCode::UnsupportedFormatVersion.into()),
    };
    let mut next = byte(&mut reader)?;
    while next.is_some_and(|b| b.is_ascii_whitespace()) { next = byte(&mut reader)?; }
    if next != Some(b'[') { return Err(ImportErrorCode::InvalidSchema.into()); }

    let mut buffer: Vec<u8> = Vec::new();
    let (mut depth, mut in_string, mut escaped) = (1usize, false, false);
    let mut first = true;
    loop {
        let Some(b) = byte(&mut reader)? else { return Err(ImportErrorCode::InvalidSchema.into()) };
        if in_string {
            buffer.push(b);
            if escaped { escaped = false } else if b == b'\\' { escaped = true } else if b == b'"' { in_string = false }
        } else if depth == 1 && buffer.is_empty() && b.is_ascii_whitespace() {
            continue;
        } else if depth == 1 && (b == b',' || b == b']') {
            let ends = b == b']';
            if buffer.iter().all(u8::is_ascii_whitespace) {
                // `[]` is fine; `[,` / `[1,]` / `[1,,2]` are not.
                if !(ends && first) { return Err(ImportErrorCode::InvalidSchema.into()); }
            } else {
                element(dataset, &buffer)?;
                buffer.clear();
            }
            first = false;
            if ends { break; }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > limits::MAX_JSON_NESTING { return Err(ImportErrorCode::ResourceLimit.into()); }
                }
                // A closer at array level that is not the array's own `]`.
                b'}' if depth == 1 => return Err(ImportErrorCode::InvalidSchema.into()),
                b'}' | b']' => depth -= 1,
                _ => {}
            }
            buffer.push(b);
            if buffer.len() > limits::MAX_JSON_ELEMENT_BYTES { return Err(ImportErrorCode::ResourceLimit.into()); }
        }
    }
    let mut suffix = Vec::new();
    while let Some(b) = byte(&mut reader)? {
        if suffix.len() >= limits::MAX_JS_PREFIX_BYTES { return Err(ImportErrorCode::UnsupportedFormatVersion.into()); }
        suffix.push(b);
    }
    let suffix = std::str::from_utf8(&suffix)
        .map_err(|_| ImportError::from(ImportErrorCode::InvalidEncoding))?.trim();
    if !suffix.is_empty() && suffix != ";" {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    Ok(dataset)
}

fn element_value(bytes: &[u8]) -> Result<Value, ImportError> {
    serde_json::from_slice(bytes).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))
}

fn exact_id(value: &Value) -> Result<String, ImportError> {
    if let Some(string) = value.get("id_str").and_then(Value::as_str) {
        if string.is_empty() || !string.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ImportErrorCode::InvalidRecord.into());
        }
        if let Some(number) = value.get("id") {
            let number = number.as_u64().ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
            if number.to_string() != string {
                return Err(ImportErrorCode::InvalidRecord.into());
            }
        }
        return Ok(string.into());
    }
    value.get("id").and_then(Value::as_u64).map(|number| number.to_string())
        .ok_or_else(|| ImportErrorCode::InvalidRecord.into())
}

fn valid_reference(value: Option<&Value>) -> Result<Option<String>, ImportError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) => Ok(Some(id.clone())),
        Some(Value::Number(number)) => number.as_u64().map(|id| Some(id.to_string()))
            .ok_or_else(|| ImportErrorCode::InvalidRecord.into()),
        _ => Err(ImportErrorCode::InvalidRecord.into()),
    }
}

fn status_id(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://x.com/").or_else(|| url.strip_prefix("https://twitter.com/"))?;
    let (_, id) = rest.rsplit_once("/status/")?;
    let id = id.split(['?', '#', '/']).next()?;
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then(|| id.to_owned())
}

fn decode_minimal_html(value: &str) -> String {
    value.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'")
        .replace("&nbsp;", "\u{a0}").replace("&eacute;", "é")
}

fn normalize_text(value: &str) -> Result<String, ImportError> {
    if value.len() > limits::MAX_TEXT_FIELD_BYTES {
        return Err(ImportErrorCode::ResourceLimit.into());
    }
    Ok(decode_minimal_html(&value.replace("\r\n", "\n").replace('\r', "\n")).nfc().collect())
}

/// One normalized tweet plus its edit chain, before cross-record resolution.
#[derive(Debug, Clone)]
pub struct ParsedTweet {
    pub record: NormalizedArchiveRecord,
    pub chain: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Note {
    created_at: DateTime<Utc>,
    text: String,
}

fn edit_chain(tweet: &Value, revision_id: &str) -> Result<Option<Vec<String>>, ImportError> {
    let ids = tweet.get("edit_control")
        .or_else(|| tweet.get("editControl"))
        .and_then(|control| control.get("edit_tweet_ids").or_else(|| control.get("editTweetIds")));
    let chain = ids.map(|ids| {
        ids.as_array().ok_or_else(|| ImportError::from(ImportErrorCode::InvalidEditChain))?
            .iter()
            .map(|value| value.as_str()
                .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                .map(str::to_owned)
                .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidEditChain)))
            .collect::<Result<Vec<_>, _>>()
    }).transpose()?;
    if chain.as_ref().is_some_and(|ids| ids.is_empty() || !ids.iter().any(|id| id == revision_id)) {
        return Err(ImportErrorCode::InvalidEditChain.into());
    }
    Ok(chain)
}

/// Normalizes one tweet envelope. `ordinal` is 1-based within `source`.
pub fn parse_tweet(envelope: &Value, ordinal: u64, import_id: Uuid, source: &str, family: FormatFamily, deleted_source: bool) -> Result<ParsedTweet, ImportError> {
    let tweet = envelope.get("tweet").unwrap_or(envelope);
    let revision_id = exact_id(tweet)?;
    let created_at = tweet.get("created_at").and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_str(value, "%a %b %d %H:%M:%S %z %Y").ok())
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let chain = edit_chain(tweet, &revision_id)?;
    let logical_id = chain.as_ref().map(|ids| ids[0].clone()).unwrap_or_else(|| revision_id.clone());
    let raw_text = if deleted_source { None } else {
        Some(tweet.get("full_text").or_else(|| tweet.get("text")).and_then(Value::as_str)
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?)
    };
    let mut body = raw_text.map(normalize_text).transpose()?;
    if body.as_ref().is_some_and(|text| text.is_empty()) { return Err(ImportErrorCode::InvalidRecord.into()); }
    let repost = !deleted_source && (tweet.get("retweeted_status").is_some()
        || body.as_ref().is_some_and(|text| text.starts_with("RT @")));
    let reply = valid_reference(tweet.get("in_reply_to_status_id_str").or_else(|| tweet.get("in_reply_to_status_id")))?;
    let mut quote_candidates = Vec::new();
    let mut quote_token = None;
    if !repost && reply.is_none() {
        if let Some(urls) = tweet.pointer("/entities/urls").and_then(Value::as_array) {
            for url in urls {
                if let (Some(token), Some(expanded)) = (url.get("url").and_then(Value::as_str), url.get("expanded_url").and_then(Value::as_str)) {
                    if let Some(id) = status_id(expanded) {
                        if id != revision_id { quote_candidates.push(id); quote_token = Some(token.to_owned()); }
                    }
                }
            }
        }
    }
    quote_candidates.sort();
    quote_candidates.dedup();
    let quoted = (quote_candidates.len() == 1).then(|| quote_candidates[0].clone());
    if repost {
        if let Some(text) = body.as_mut() {
            if let Some(colon) = text.strip_prefix("RT @").and_then(|rest| rest.find(':').map(|index| index + 4)) {
                *text = text[colon + 1..].trim_start().to_owned();
            }
        }
    } else if quoted.is_some() {
        if let (Some(text), Some(token)) = (body.as_mut(), quote_token.as_deref()) {
            *text = text.replace(token, "");
            *text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    let (record_type, authorship, parent_source_id, relation_confidence) = if repost {
        (RecordType::Repost, Authorship::Reshared, None, RelationConfidence::None)
    } else if let Some(parent) = reply {
        (RecordType::Reply, Authorship::OwnerAuthored, Some(parent), RelationConfidence::Explicit)
    } else if quoted.is_some() {
        (RecordType::Quote, Authorship::OwnerAuthored, None, RelationConfidence::Heuristic)
    } else {
        (RecordType::Post, Authorship::OwnerAuthored, None, RelationConfidence::None)
    };
    Ok(ParsedTweet {
        chain,
        record: NormalizedArchiveRecord {
            schema_version: 1, import_id, platform: ArchivePlatform::X,
            source_logical_id: logical_id, source_revision_id: revision_id,
            record_type, text_format: TextFormat::Standard, authorship,
            state: if deleted_source { ContentState::Deleted } else { ContentState::Active },
            created_at: Some(created_at), title: None, body: if deleted_source { None } else { body },
            parent_source_id, quoted_source_id: quoted, relation_confidence, text_truncated: false,
            provenance: RecordProvenance {
                source_file: source.into(), source_ordinal: ordinal,
                format_family: family, format_confidence: FormatConfidence::Compatible,
            },
        },
    })
}

pub fn parse_note(envelope: &Value) -> Result<Note, ImportError> {
    let note = envelope.get("noteTweet").or_else(|| envelope.get("note_tweet")).unwrap_or(envelope);
    let created_at = note.get("createdAt").and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok()).map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let text = note.pointer("/core/text").or_else(|| note.get("text")).and_then(Value::as_str)
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord)).and_then(normalize_text)?;
    if text.is_empty() { return Err(ImportErrorCode::InvalidRecord.into()); }
    Ok(Note { created_at, text })
}

/// Item handed from the blocking archive reader to the async resolver.
#[derive(Debug)]
pub enum XItem {
    Note(Note),
    Tweet(ParsedTweet),
}

/// Streams one X entry, emitting notes or tweets. Tweets from a
/// `deleted-tweets` file (by name or wrapper) become bodyless tombstones.
pub fn stream_file<R: Read>(
    reader: R,
    import_id: Uuid,
    source: &str,
    family: FormatFamily,
    emit: &mut dyn FnMut(XItem) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let mut ordinal = 0u64;
    read_js_array(reader, |dataset, bytes| {
        ordinal += 1;
        if ordinal as usize > limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
        let value = element_value(bytes)?;
        if dataset == Dataset::NoteTweet {
            return emit(XItem::Note(parse_note(&value)?));
        }
        let deleted = source.ends_with("deleted-tweets.js") || dataset == Dataset::DeletedTweets;
        emit(XItem::Tweet(parse_tweet(&value, ordinal, import_id, source, family, deleted)?))
    })?;
    Ok(())
}

/// Receives resolved X records. Production stages into Postgres; pure parsing
/// collects in memory.
#[async_trait::async_trait]
pub trait RecordSink: Send {
    /// Stage `record`, replacing any earlier record with the same revision.
    async fn stage(&mut self, record: &NormalizedArchiveRecord) -> Result<(), ImportError>;
    async fn unstage(&mut self, source_revision_id: &str) -> Result<(), ImportError>;
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RevisionMeta {
    deleted: bool,
    head: Option<String>,
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, ImportError> {
    serde_json::to_vec(value).map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ImportError> {
    serde_json::from_slice(bytes).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))
}

/// Cross-record X rules with bounded memory:
/// - previews ending in `…` join exactly one long-form note with the same timestamp;
/// - a revision seen twice: a deleted copy wins, two active copies conflict;
/// - every edit chain must be complete and identical for its head, and only
///   the chain's last revision is kept.
///
/// Notes must all be added before the first tweet (the reader streams
/// `note-tweet.js` first).
pub struct XResolver<'p> {
    notes: BoundedIndex<'p>,
    revisions: BoundedIndex<'p>,
    chains: BoundedIndex<'p>,
    parsed: usize,
}

impl<'p> XResolver<'p> {
    pub fn new(spill: Option<SpillTarget<'p>>) -> Self {
        Self {
            notes: BoundedIndex::new(spill, "x_note_ts"),
            revisions: BoundedIndex::new(spill, "x_revision"),
            chains: BoundedIndex::new(spill, "x_edit_chain"),
            parsed: 0,
        }
    }

    fn note_key(created_at: &DateTime<Utc>) -> String {
        created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
    }

    pub async fn add_note(&mut self, note: Note) -> Result<(), ImportError> {
        let key = Self::note_key(&note.created_at);
        let mut candidates: Vec<Note> = match self.notes.get(&key).await? {
            Some(payload) => decode(&payload)?,
            None => Vec::new(),
        };
        candidates.push(note);
        self.notes.insert(key, encode(&candidates)?).await
    }

    pub async fn add_tweet(&mut self, mut item: ParsedTweet, sink: &mut dyn RecordSink) -> Result<(), ImportError> {
        self.parsed += 1;
        if self.parsed > limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
        if item.record.state == ContentState::Active {
            if let Some(preview) = item.record.body.as_deref().and_then(|text| text.strip_suffix('…')) {
                let created_at = item.record.created_at.ok_or(ImportErrorCode::AmbiguousNoteJoin)?;
                let candidates: Vec<Note> = match self.notes.get(&Self::note_key(&created_at)).await? {
                    Some(payload) => decode(&payload)?,
                    None => Vec::new(),
                };
                let matches: Vec<&Note> = candidates.iter().filter(|note| note.text.starts_with(preview) && note.text.len() > preview.len()).collect();
                if matches.len() != 1 { return Err(ImportErrorCode::AmbiguousNoteJoin.into()); }
                item.record.body = Some(matches[0].text.clone());
                item.record.text_format = TextFormat::LongFormNote;
            }
        }
        if let Some(chain) = &item.chain {
            match self.chains.get(&chain[0]).await? {
                Some(existing) if decode::<Vec<String>>(&existing)? != *chain => return Err(ImportErrorCode::InvalidEditChain.into()),
                Some(_) => {}
                None => self.chains.insert(chain[0].clone(), encode(chain)?).await?,
            }
        }
        let revision = item.record.source_revision_id.clone();
        let deleted = item.record.state == ContentState::Deleted;
        match self.revisions.get(&revision).await? {
            Some(existing) if decode::<RevisionMeta>(&existing)?.deleted => return Ok(()),
            Some(_) if !deleted => return Err(ImportErrorCode::DuplicateConflict.into()),
            _ => {}
        }
        let head = item.chain.as_ref().map(|chain| chain[0].clone());
        self.revisions.insert(revision, encode(&RevisionMeta { deleted, head })?).await?;
        sink.stage(&item.record).await
    }

    pub async fn finish(self, sink: &mut dyn RecordSink) -> Result<(), ImportError> {
        let mut cursor = IndexCursor::default();
        loop {
            let page = self.chains.next_page(&mut cursor).await?;
            if page.is_empty() { break; }
            for (_, payload) in page {
                for id in decode::<Vec<String>>(&payload)? {
                    if self.revisions.get(&id).await?.is_none() { return Err(ImportErrorCode::InvalidEditChain.into()); }
                }
            }
        }
        let mut cursor = IndexCursor::default();
        loop {
            let page = self.revisions.next_page(&mut cursor).await?;
            if page.is_empty() { break; }
            for (revision, payload) in page {
                let Some(head) = decode::<RevisionMeta>(&payload)?.head else { continue };
                let chain: Vec<String> = decode(&self.chains.get(&head).await?.ok_or(ImportErrorCode::InvalidEditChain)?)?;
                if chain.last() != Some(&revision) {
                    sink.unstage(&revision).await?;
                }
            }
        }
        Ok(())
    }
}

/// In-memory sink: the resolved record set in archive order.
#[derive(Default)]
pub struct CollectSink {
    records: BTreeMap<String, NormalizedArchiveRecord>,
}

#[async_trait::async_trait]
impl RecordSink for CollectSink {
    async fn stage(&mut self, record: &NormalizedArchiveRecord) -> Result<(), ImportError> {
        self.records.insert(record.source_revision_id.clone(), record.clone());
        Ok(())
    }
    async fn unstage(&mut self, source_revision_id: &str) -> Result<(), ImportError> {
        self.records.remove(source_revision_id);
        Ok(())
    }
}

impl CollectSink {
    pub fn into_records(self) -> Vec<NormalizedArchiveRecord> {
        let mut output: Vec<_> = self.records.into_values().collect();
        output.sort_by(|a, b| a.provenance.source_file.cmp(&b.provenance.source_file)
            .then(a.provenance.source_ordinal.cmp(&b.provenance.source_ordinal)));
        output
    }
}

/// Parses a single X file with no database: the same streaming reader and
/// resolver as production, with memory-only indexes.
pub async fn parse_tweets(input: &[u8], import_id: Uuid, source: &str, family: FormatFamily) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
    let mut items = Vec::new();
    stream_file(input, import_id, source, family, &mut |item| { items.push(item); Ok(()) })?;
    let mut resolver = XResolver::new(None);
    let mut sink = CollectSink::default();
    for item in items {
        match item {
            XItem::Note(note) => resolver.add_note(note).await?,
            XItem::Tweet(tweet) => resolver.add_tweet(tweet, &mut sink).await?,
        }
    }
    resolver.finish(&mut sink).await?;
    Ok(sink.into_records())
}
