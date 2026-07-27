use super::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

pub fn unwrap_js(input: &[u8]) -> Result<&[u8], ImportError> {
    let prefix = &input[..input.len().min(limits::MAX_JS_PREFIX_BYTES)];
    let eq = prefix.iter().position(|byte| *byte == b'=')
        .ok_or_else(|| ImportError::from(ImportErrorCode::UnsupportedFormatVersion))?;
    let lhs = std::str::from_utf8(&prefix[..eq])
        .map_err(|_| ImportError::from(ImportErrorCode::InvalidEncoding))?.trim();
    let Some(name) = lhs.strip_prefix("window.YTD.") else {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    };
    let Some((dataset, part)) = name.rsplit_once(".part") else {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    };
    if !matches!(dataset, "tweets" | "tweet" | "deleted_tweets" | "note_tweet")
        || part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    let rest = &input[eq + 1..];
    let start = rest.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(rest.len());
    if rest.get(start) != Some(&b'[') {
        return Err(ImportErrorCode::InvalidSchema.into());
    }
    let end = rest.iter().rposition(|byte| *byte == b']')
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidSchema))?;
    let suffix = std::str::from_utf8(&rest[end + 1..])
        .map_err(|_| ImportError::from(ImportErrorCode::InvalidEncoding))?.trim();
    if !suffix.is_empty() && suffix != ";" {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    Ok(&rest[start..=end])
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

fn nesting_within_limit(input: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for &byte in input {
        if string {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { string = false; }
        } else if byte == b'"' {
            string = true;
        } else if matches!(byte, b'{' | b'[') {
            depth += 1;
            if depth > limits::MAX_JSON_NESTING { return false; }
        } else if matches!(byte, b'}' | b']') {
            depth = depth.saturating_sub(1);
        }
    }
    true
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

#[derive(Clone)]
struct Parsed {
    record: NormalizedArchiveRecord,
    chain: Option<Vec<String>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Note {
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

fn parse_file(input: &[u8], import_id: Uuid, source: &str, family: FormatFamily) -> Result<Vec<Parsed>, ImportError> {
    let data = unwrap_js(input)?;
    if !nesting_within_limit(data) { return Err(ImportErrorCode::ResourceLimit.into()); }
    let rows: Vec<Value> = serde_json::from_slice(data).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?;
    if rows.len() > limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
    let deleted_source = source.ends_with("deleted-tweets.js")
        || std::str::from_utf8(input).is_ok_and(|text| text.trim_start().starts_with("window.YTD.deleted_tweets."));
    let mut parsed = Vec::with_capacity(rows.len());
    for (ordinal, envelope) in rows.into_iter().enumerate() {
        let tweet = envelope.get("tweet").unwrap_or(&envelope);
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
        parsed.push(Parsed {
            chain,
            record: NormalizedArchiveRecord {
                schema_version: 1, import_id, platform: ArchivePlatform::X,
                source_logical_id: logical_id, source_revision_id: revision_id,
                record_type, text_format: TextFormat::Standard, authorship,
                state: if deleted_source { ContentState::Deleted } else { ContentState::Active },
                created_at: Some(created_at), title: None, body: if deleted_source { None } else { body },
                parent_source_id, quoted_source_id: quoted, relation_confidence, text_truncated: false,
                provenance: RecordProvenance {
                    source_file: source.into(), source_ordinal: (ordinal + 1) as u64,
                    format_family: family, format_confidence: FormatConfidence::Compatible,
                },
            },
        });
    }
    Ok(parsed)
}

fn coalesce(parsed: Vec<Parsed>) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
    let available: HashSet<String> = parsed.iter().map(|item| item.record.source_revision_id.clone()).collect();
    let mut chains: HashMap<String, Vec<String>> = HashMap::new();
    for item in &parsed {
        if let Some(chain) = &item.chain {
            if chain.iter().any(|id| !available.contains(id))
                || chains.insert(chain[0].clone(), chain.clone()).is_some_and(|old| old != *chain)
            { return Err(ImportErrorCode::InvalidEditChain.into()); }
        }
    }
    let mut by_revision: HashMap<String, Parsed> = HashMap::new();
    for item in parsed {
        let revision = item.record.source_revision_id.clone();
        match by_revision.get(&revision) {
            Some(existing) if existing.record.state == ContentState::Deleted => continue,
            Some(_) if item.record.state != ContentState::Deleted => return Err(ImportErrorCode::DuplicateConflict.into()),
            _ => { by_revision.insert(revision, item); }
        }
    }
    let mut output = Vec::new();
    for (_, item) in by_revision {
        if let Some(chain) = &item.chain {
            if item.record.source_revision_id != *chain.last().ok_or(ImportErrorCode::InvalidEditChain)? { continue; }
        }
        output.push(item.record);
    }
    output.sort_by(|a, b| a.provenance.source_file.cmp(&b.provenance.source_file)
        .then(a.provenance.source_ordinal.cmp(&b.provenance.source_ordinal)));
    Ok(output)
}

fn parse_notes(input: &[u8]) -> Result<Vec<Note>, ImportError> {
    let data = unwrap_js(input)?;
    if !nesting_within_limit(data) { return Err(ImportErrorCode::ResourceLimit.into()); }
    let rows: Vec<Value> = serde_json::from_slice(data).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?;
    if rows.len() > limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
    rows.into_iter().map(|envelope| {
        let note = envelope.get("noteTweet").or_else(|| envelope.get("note_tweet")).unwrap_or(&envelope);
        let created_at = note.get("createdAt").and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()).map(|date| date.with_timezone(&Utc))
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord))?;
        let text = note.pointer("/core/text").or_else(|| note.get("text")).and_then(Value::as_str)
            .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidRecord)).and_then(normalize_text)?;
        if text.is_empty() { return Err(ImportErrorCode::InvalidRecord.into()); }
        Ok(Note { created_at, text })
    }).collect()
}

pub fn parse_tweets(input: &[u8], import_id: Uuid, source: &str, family: FormatFamily) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
    coalesce(parse_file(input, import_id, source, family)?)
}

pub async fn parse_archive(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    import_id: Uuid,
    files: &[(String, Vec<u8>)],
    family: FormatFamily,
) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
    let mut notes = super::staging::BoundedIndex::default();
    for (name, input) in files {
        if name.ends_with("note-tweet.js") {
            for note in parse_notes(input)? {
                let key = note.created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
                let mut candidates: Vec<Note> = match notes.get(pool, tenant_id, import_id, "x_note_ts", &key).await? {
                    Some(payload) => serde_json::from_slice(&payload).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?,
                    None => Vec::new(),
                };
                candidates.push(note);
                let payload = serde_json::to_vec(&candidates).map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))?;
                notes.insert(pool, tenant_id, import_id, "x_note_ts", key, payload).await?;
            }
        }
    }
    let mut parsed = Vec::new();
    for (name, input) in files {
        if name.ends_with("note-tweet.js") || name.ends_with("manifest.js") { continue; }
        let mut file = parse_file(input, import_id, name, family)?;
        for item in &mut file {
            if item.record.state != ContentState::Active { continue; }
            let Some(preview) = item.record.body.as_deref().and_then(|text| text.strip_suffix('…')) else { continue; };
            let created_at = item.record.created_at.ok_or(ImportErrorCode::AmbiguousNoteJoin)?;
            let key = created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
            let candidates: Vec<Note> = match notes.get(pool, tenant_id, import_id, "x_note_ts", &key).await? {
                Some(payload) => serde_json::from_slice(&payload).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?,
                None => Vec::new(),
            };
            let matches: Vec<&Note> = candidates.iter().filter(|note| note.text.starts_with(preview) && note.text.len() > preview.len()).collect();
            if matches.len() != 1 { return Err(ImportErrorCode::AmbiguousNoteJoin.into()); }
            item.record.body = Some(matches[0].text.clone());
            item.record.text_format = TextFormat::LongFormNote;
        }
        parsed.extend(file);
        if parsed.len() > limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
    }
    let mut edit_index = super::staging::BoundedIndex::default();
    for item in &parsed {
        if let Some(chain) = &item.chain {
            let payload = serde_json::to_vec(chain).map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))?;
            edit_index.insert(pool, tenant_id, import_id, "x_edit_chain", chain[0].clone(), payload).await?;
        }
    }
    coalesce(parsed)
}
