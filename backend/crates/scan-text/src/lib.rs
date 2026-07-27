//! Shared scan text minimization, segmentation, packing, and aggregation.
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_MAX_ITEMS: usize = 16;
pub const DEFAULT_MAX_TOKENS: u32 = 12_000;
pub const DEFAULT_MAX_SCALARS: usize = 6_000;
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorship {
    Authored,
    Amplified,
}

impl Authorship {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::Amplified => "amplified",
        }
    }

    /// Map durable DB values (`owner_authored`|`reshared`) or model values.
    pub fn from_storage(s: &str) -> Option<Self> {
        match s {
            "authored" | "owner_authored" => Some(Self::Authored),
            "amplified" | "reshared" => Some(Self::Amplified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NormalizedRow {
    pub content_item_id: Uuid,
    pub platform: String,
    pub kind: String,
    pub authorship: Authorship,
    pub text: String,
    pub content_hmac: Vec<u8>,
    pub source_logical_id: String,
    pub source_order: u64,
}

#[derive(Debug, Clone)]
pub struct BatchHmacKey {
    pub id: String,
    pub secret: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BatchLimits {
    pub max_items: usize,
    pub max_tokens: u32,
    pub max_scalars: usize,
    pub max_output_tokens: u32,
    pub system_prompt_token_estimate: u32,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_items: DEFAULT_MAX_ITEMS,
            max_tokens: DEFAULT_MAX_TOKENS,
            max_scalars: DEFAULT_MAX_SCALARS,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            system_prompt_token_estimate: 2_500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackedItem {
    pub item_index: u32,
    pub platform: String,
    pub kind: String,
    pub authorship: Authorship,
    pub text: String,
    pub content_item_id: Uuid,
    pub segment_ordinal: u32,
    pub content_hmac: Vec<u8>,
    pub source_logical_id: String,
}

#[derive(Debug, Clone)]
pub struct PackedBatch {
    pub batch_ordinal: u32,
    pub batch_id: String,
    pub batch_key_id: String,
    pub items: Vec<PackedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AggregateReason {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AggregateEvidence {
    pub text: String,
    pub supports_reason_code: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentResult {
    pub content_item_id: Uuid,
    pub segment_ordinal: u32,
    pub decision: String,
    pub risk: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub reasons: Vec<AggregateReason>,
    pub evidence: Vec<AggregateEvidence>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceAggregate {
    pub content_item_id: Uuid,
    pub decision: String,
    pub risk: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub reasons: Vec<AggregateReason>,
    pub evidence: Vec<AggregateEvidence>,
}

/// Decode HTML entities, convert br/block to newline, strip tags, drop control
/// chars (keep `\n`/`\t`), replace URLs with `<url>`, NFC, collapse horizontal WS,
/// cap blank-line runs at one, trim.
pub fn minimize_text(raw: &str) -> String {
    let mut s = decode_entities(raw);
    // <br> / block boundaries -> newline
    for pat in [
        "<br>", "<br/>", "<br />", "<BR>", "<BR/>", "<BR />", "<Br>", "<Br/>",
    ] {
        s = s.replace(pat, "\n");
    }
    for pat in [
        "</p>", "</div>", "</li>", "</tr>", "</h1>", "</h2>", "</h3>", "</h4>",
        "</blockquote>", "</pre>", "</section>", "</article>",
    ] {
        s = s.replace(pat, "\n");
    }
    // strip remaining tags
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' && in_tag {
            in_tag = false;
            continue;
        }
        if !in_tag {
            out.push(ch);
        }
    }
    s = out;
    // control chars except \n \t
    s = s
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    // URLs -> <url>
    s = replace_urls(&s);
    // NFC
    s = s.nfc().collect::<String>();
    // collapse horizontal whitespace
    s = collapse_horizontal(&s);
    // cap blank-line runs at one
    s = collapse_blank_lines(&s);
    s.trim().to_string()
}

fn decode_entities(s: &str) -> String {
    let mut out = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    // numeric entities
    while let Some(start) = out.find("&#") {
        let rest = &out[start + 2..];
        let (digits, is_hex) = if rest.starts_with('x') || rest.starts_with('X') {
            (&rest[1..], true)
        } else {
            (rest, false)
        };
        if let Some(end_rel) = digits.find(';') {
            let num = &digits[..end_rel];
            let code = if is_hex {
                u32::from_str_radix(num, 16).ok()
            } else {
                num.parse().ok()
            };
            if let Some(cp) = code.and_then(char::from_u32) {
                let before = &out[..start];
                let after = &digits[end_rel + 1..];
                out = format!("{before}{cp}{after}");
                continue;
            }
        }
        break;
    }
    out
}

fn replace_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if looks_like_url(&s[i..]) {
            out.push_str("<url>");
            while i < bytes.len() && !s[i..].chars().next().unwrap().is_whitespace() {
                let ch = s[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
}

fn collapse_horizontal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == '\n' {
            out.push(ch);
            prev_space = false;
        } else if ch == '\t' || ch == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0;
    for line in s.split('\n') {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line.trim_end());
        }
    }
    out
}

/// Unicode scalar count.
pub fn scalar_len(s: &str) -> usize {
    s.chars().count()
}

/// Split without overlapping; prefer paragraph, sentence, word, grapheme.
pub fn segment_text(text: &str, max_scalars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    if scalar_len(text) <= max_scalars {
        return vec![text.to_string()];
    }
    let mut segments = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if scalar_len(remaining) <= max_scalars {
            segments.push(remaining.to_string());
            break;
        }
        let (chunk, rest) = split_once(remaining, max_scalars);
        if chunk.is_empty() {
            // force one grapheme
            if let Some(g) = remaining.graphemes(true).next() {
                segments.push(g.to_string());
                remaining = &remaining[g.len()..];
                continue;
            }
            break;
        }
        segments.push(chunk.to_string());
        remaining = rest;
    }
    segments
}

fn split_once<'a>(text: &'a str, max_scalars: usize) -> (&'a str, &'a str) {
    let cut = byte_index_at_scalar(text, max_scalars);
    // Prefer paragraph, then sentence, then word; never drop scalars (no trim).
    if let Some(idx) = text[..cut].rfind("\n\n") {
        let end = idx + 2;
        return (&text[..end], &text[end..]);
    }
    if let Some(idx) = rfind_sentence_boundary(&text[..cut]) {
        let end = idx + 1;
        // include following single space with the left chunk when present
        let end = if text[end..].starts_with(' ') {
            end + 1
        } else {
            end
        };
        return (&text[..end], &text[end..]);
    }
    if let Some(idx) = text[..cut].rfind(|c: char| c.is_whitespace()) {
        // keep the whitespace run with the right segment
        let mut start_ws = idx;
        let bytes = text.as_bytes();
        while start_ws > 0 {
            let ch = text[..start_ws].chars().next_back().unwrap();
            if ch.is_whitespace() && ch != '\n' {
                start_ws -= ch.len_utf8();
            } else {
                break;
            }
        }
        // avoid empty left when only leading ws in window
        if start_ws == 0 {
            let cut = grapheme_safe_cut(text, max_scalars);
            return (&text[..cut], &text[cut..]);
        }
        let _ = bytes;
        return (&text[..start_ws], &text[start_ws..]);
    }
    let cut = grapheme_safe_cut(text, max_scalars);
    (&text[..cut], &text[cut..])
}

fn byte_index_at_scalar(s: &str, n: usize) -> usize {
    s.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn grapheme_safe_cut(s: &str, max_scalars: usize) -> usize {
    let mut scalars = 0;
    let mut last = 0;
    for g in s.graphemes(true) {
        let g_scalars = g.chars().count();
        if scalars + g_scalars > max_scalars {
            break;
        }
        scalars += g_scalars;
        last += g.len();
    }
    if last == 0 {
        // at least one grapheme
        s.graphemes(true).next().map(|g| g.len()).unwrap_or(0)
    } else {
        last
    }
}

fn rfind_sentence_boundary(s: &str) -> Option<usize> {
    s.char_indices()
        .rev()
        .find(|(_, c)| matches!(c, '.' | '!' | '?' | '。' | '！' | '？'))
        .map(|(i, _)| i)
}

/// Deterministic rough token estimate (~4 chars / token, min 1 for non-empty).
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let chars = scalar_len(text) as u32;
    chars.div_ceil(4).max(1)
}

pub fn compute_batch_id(
    key: &BatchHmacKey,
    scan_id: Uuid,
    batch_ordinal: u32,
    prompt_sha256: &[u8; 32],
    items: &[(/* content_hmac */ &[u8], u32 /* segment_ordinal */)],
) -> String {
    let mut mac =
        HmacSha256::new_from_slice(&key.secret).expect("HMAC key length is valid for sha256");
    mac.update(scan_id.as_bytes());
    mac.update(&batch_ordinal.to_be_bytes());
    mac.update(prompt_sha256);
    for (hmac, seg) in items {
        mac.update(hmac);
        mac.update(&seg.to_be_bytes());
    }
    let tag = mac.finalize().into_bytes();
    format!("{}.{}", key.id, hex::encode(tag))
}

struct Prepared {
    content_item_id: Uuid,
    platform: String,
    kind: String,
    authorship: Authorship,
    text: String,
    content_hmac: Vec<u8>,
    source_logical_id: String,
    segment_ordinal: u32,
    tokens: u32,
}

/// Prepare rows (minimize + segment) then greedy-pack into batches.
pub fn pack_batches(
    rows: &[NormalizedRow],
    scan_id: Uuid,
    prompt_sha256: &[u8; 32],
    key: &BatchHmacKey,
    limits: &BatchLimits,
) -> Vec<PackedBatch> {
    // stable order: source_order, source_logical_id
    let mut ordered: Vec<&NormalizedRow> = rows.iter().collect();
    ordered.sort_by(|a, b| {
        a.source_order
            .cmp(&b.source_order)
            .then_with(|| a.source_logical_id.cmp(&b.source_logical_id))
    });

    let mut prepared: Vec<Prepared> = Vec::new();
    for row in ordered {
        let mini = minimize_text(&row.text);
        if mini.is_empty() {
            continue;
        }
        let mut segs = segment_text(&mini, limits.max_scalars);
        // re-split if a segment still exceeds token budget alone
        let overhead = limits.system_prompt_token_estimate + 64;
        let item_budget = limits
            .max_tokens
            .saturating_sub(overhead)
            .max(64);
        let mut final_segs = Vec::new();
        for seg in segs.drain(..) {
            let mut stack = vec![seg];
            while let Some(s) = stack.pop() {
                let t = estimate_tokens(&s);
                if t <= item_budget {
                    final_segs.push(s);
                } else {
                    let target_scalars = ((item_budget as usize).saturating_mul(4)).max(64);
                    let smaller = segment_text(&s, target_scalars.min(scalar_len(&s).saturating_sub(1).max(1)));
                    if smaller.len() <= 1 {
                        // cannot split further; keep as-is (plan: never drop)
                        final_segs.push(s);
                    } else {
                        for part in smaller.into_iter().rev() {
                            stack.push(part);
                        }
                    }
                }
            }
        }
        for (ord, text) in final_segs.into_iter().enumerate() {
            let tokens = estimate_tokens(&text);
            prepared.push(Prepared {
                content_item_id: row.content_item_id,
                platform: row.platform.clone(),
                kind: row.kind.clone(),
                authorship: row.authorship,
                text,
                content_hmac: row.content_hmac.clone(),
                source_logical_id: row.source_logical_id.clone(),
                segment_ordinal: ord as u32,
                tokens,
            });
        }
    }

    let mut batches = Vec::new();
    let mut idx = 0usize;
    let mut ordinal = 0u32;
    while idx < prepared.len() {
        let mut items: Vec<PackedItem> = Vec::new();
        let mut token_sum = limits.system_prompt_token_estimate;
        let mut local_i = 0u32;
        while idx < prepared.len() && items.len() < limits.max_items {
            let p = &prepared[idx];
            // estimated output ~ 80 tokens/item rough
            let out_est = ((items.len() as u32) + 1) * 80;
            if out_est > limits.max_output_tokens && !items.is_empty() {
                break;
            }
            let next_tokens = token_sum + p.tokens + 24;
            if next_tokens > limits.max_tokens && !items.is_empty() {
                break;
            }
            token_sum = next_tokens.max(token_sum + 1);
            items.push(PackedItem {
                item_index: local_i,
                platform: p.platform.clone(),
                kind: p.kind.clone(),
                authorship: p.authorship,
                text: p.text.clone(),
                content_item_id: p.content_item_id,
                segment_ordinal: p.segment_ordinal,
                content_hmac: p.content_hmac.clone(),
                source_logical_id: p.source_logical_id.clone(),
            });
            local_i += 1;
            idx += 1;
        }
        if items.is_empty() {
            // force single oversized item
            let p = &prepared[idx];
            items.push(PackedItem {
                item_index: 0,
                platform: p.platform.clone(),
                kind: p.kind.clone(),
                authorship: p.authorship,
                text: p.text.clone(),
                content_item_id: p.content_item_id,
                segment_ordinal: p.segment_ordinal,
                content_hmac: p.content_hmac.clone(),
                source_logical_id: p.source_logical_id.clone(),
            });
            idx += 1;
        }
        let pairs: Vec<_> = items
            .iter()
            .map(|it| (it.content_hmac.as_slice(), it.segment_ordinal))
            .collect();
        let batch_id = compute_batch_id(key, scan_id, ordinal, prompt_sha256, &pairs);
        batches.push(PackedBatch {
            batch_ordinal: ordinal,
            batch_id,
            batch_key_id: key.id.clone(),
            items,
        });
        ordinal += 1;
    }
    batches
}

fn risk_rank(r: &str) -> u8 {
    match r {
        "high" => 4,
        "medium" => 3,
        "low" => 2,
        "none" => 1,
        _ => 0,
    }
}

/// Aggregate segment results per source content item.
pub fn aggregate_source_results(segments: &[SegmentResult]) -> Vec<SourceAggregate> {
    let mut by_source: HashMap<Uuid, Vec<&SegmentResult>> = HashMap::new();
    for s in segments {
        by_source.entry(s.content_item_id).or_default().push(s);
    }
    let mut out = Vec::new();
    for (id, mut segs) in by_source {
        segs.sort_by_key(|s| s.segment_ordinal);
        // pick winner: higher risk, then confidence, then lower segment ordinal
        let winner = segs
            .iter()
            .copied()
            .max_by(|a, b| {
                risk_rank(&a.risk)
                    .cmp(&risk_rank(&b.risk))
                    .then_with(|| {
                        a.confidence
                            .partial_cmp(&b.confidence)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| b.segment_ordinal.cmp(&a.segment_ordinal))
            })
            .expect("non-empty");
        let is_flag = !(winner.risk == "none" || winner.decision == "no_flag");
        let decision = if is_flag {
            "flag".to_string()
        } else {
            "no_flag".to_string()
        };
        let category = winner.category.clone();
        // collect unique reasons/evidence for winning category, source order, cap 3
        let mut reasons = Vec::new();
        let mut evidence = Vec::new();
        let mut seen_r = std::collections::HashSet::new();
        let mut seen_e = std::collections::HashSet::new();
        for s in &segs {
            if s.category != category && is_flag {
                continue;
            }
            for r in &s.reasons {
                if seen_r.insert(r.code.clone()) {
                    reasons.push(r.clone());
                }
            }
            for e in &s.evidence {
                let key = format!("{}|{}", e.supports_reason_code, e.text);
                if seen_e.insert(key) {
                    evidence.push(e.clone());
                }
            }
        }
        reasons.truncate(3);
        evidence.truncate(3);
        if !is_flag {
            reasons.clear();
            evidence.clear();
        }
        out.push(SourceAggregate {
            content_item_id: id,
            decision,
            risk: winner.risk.clone(),
            category: if is_flag { category } else { None },
            confidence: winner.confidence,
            reasons,
            evidence,
        });
    }
    out.sort_by_key(|a| a.content_item_id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_nfc_entities_urls_controls() {
        let raw =
            "caf\u{0065}\u{0301} &amp; <b>hi</b>\r\nhttps://example.com/a?x=1\u{0007}  spaces";
        let out = minimize_text(raw);
        assert!(out.contains('\u{00e9}'), "expected NFC é in {out:?}");
        assert!(!out.contains('\u{0301}'));
        assert!(out.contains('&'));
        assert!(out.contains("hi"));
        assert!(out.contains("<url>"));
        assert!(!out.contains("https://"));
        assert!(!out.contains('\u{0007}'));
    }

    #[test]
    fn minimize_strips_tags_and_urls() {
        let t = minimize_text("Hello <b>world</b> see https://example.com/x now");
        assert!(t.contains("Hello world"));
        assert!(t.contains("<url>"));
        assert!(!t.contains("https://"));
    }

    #[test]
    fn minimize_br_and_blank_line_cap() {
        let out = minimize_text("a<br>b<br/><p>c</p>\n\n\n\nd");
        assert!(out.contains('a') && out.contains('b') && out.contains('c') && out.contains('d'));
        assert!(!out.contains("\n\n\n"));
    }

    #[test]
    fn grapheme_boundary_never_splits_zwj_emoji() {
        let emoji = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let text = format!("hello {emoji} world and more text here");
        let segs = segment_text(&text, 10);
        assert_eq!(segs.concat(), text);
        for s in &segs {
            assert!(
                !s.ends_with('\u{200d}'),
                "segment must not end inside ZWJ sequence: {s:?}"
            );
            if s.contains('👨') || s.contains('👩') || s.contains('👧') || s.contains('👦') {
                assert!(s.contains(emoji), "partial family emoji in segment: {s:?}");
            }
        }
    }

    #[test]
    fn combining_mark_stays_with_base() {
        let text = "a\u{0301}bcdefghij";
        let segs = segment_text(text, 2);
        assert_eq!(segs.concat(), text);
        assert!(
            segs[0].starts_with("a\u{0301}"),
            "combining acute split from base: {:?}",
            segs[0]
        );
    }

    #[test]
    fn segment_respects_max() {
        let s = "a".repeat(100);
        let segs = segment_text(&s, 30);
        assert!(segs.len() >= 3);
        for seg in &segs {
            assert!(scalar_len(seg) <= 30);
        }
        assert_eq!(segs.concat(), s);
    }

    #[test]
    fn pack_deterministic_batch_id() {
        let key = BatchHmacKey {
            id: "local_v1".into(),
            secret: vec![1u8; 32],
        };
        let scan = Uuid::nil();
        let prompt = [2u8; 32];
        let rows: Vec<NormalizedRow> = (0..5)
            .map(|i| NormalizedRow {
                content_item_id: Uuid::from_u128(i as u128 + 1),
                platform: "x".into(),
                kind: "tweet".into(),
                authorship: Authorship::Authored,
                text: format!("hello there friend {i}"),
                content_hmac: vec![9u8.wrapping_add(i as u8); 32],
                source_logical_id: format!("a{i}"),
                source_order: i as u64,
            })
            .collect();
        let limits = BatchLimits {
            max_items: 2,
            ..BatchLimits::default()
        };
        let b1 = pack_batches(&rows, scan, &prompt, &key, &limits);
        let b2 = pack_batches(&rows, scan, &prompt, &key, &limits);
        assert_eq!(b1.len(), b2.len());
        assert!(b1.len() >= 3);
        for (a, b) in b1.iter().zip(b2.iter()) {
            assert_eq!(a.batch_id, b.batch_id);
            assert_eq!(a.batch_ordinal, b.batch_ordinal);
            assert_eq!(a.items.len(), b.items.len());
            for (ia, ib) in a.items.iter().zip(b.items.iter()) {
                assert_eq!(ia.item_index, ib.item_index);
                assert_eq!(ia.text, ib.text);
            }
        }
        assert_eq!(b1[0].items[0].item_index, 0);
        assert_eq!(b1[0].items[1].item_index, 1);
        assert_eq!(b1[1].items[0].item_index, 0);
        assert!(b1[0].batch_id.starts_with("local_v1."));
        assert_eq!(b1[0].batch_id.len(), "local_v1.".len() + 64);
    }

    #[test]
    fn batch_id_stable_and_ordinal_sensitive() {
        let key = BatchHmacKey {
            id: "k1".into(),
            secret: b"test-batch-hmac-key-32bytes-long!!".to_vec(),
        };
        let scan = Uuid::from_u128(7);
        let prompt = [0xabu8; 32];
        let hmac = [1u8; 32];
        let items = [(hmac.as_slice(), 0u32)];
        let a = compute_batch_id(&key, scan, 0, &prompt, &items);
        let b = compute_batch_id(&key, scan, 1, &prompt, &items);
        assert_ne!(a, b);
        assert_eq!(a, compute_batch_id(&key, scan, 0, &prompt, &items));
        assert!(a.starts_with("k1."));
        assert_eq!(a.strip_prefix("k1.").unwrap().len(), 64);
    }

    #[test]
    fn token_cap_resplit_without_truncation() {
        let long = "word ".repeat(800);
        let row = NormalizedRow {
            content_item_id: Uuid::from_u128(42),
            platform: "reddit".into(),
            kind: "comment".into(),
            authorship: Authorship::Authored,
            text: long.clone(),
            content_hmac: vec![3u8; 32],
            source_logical_id: "long".into(),
            source_order: 0,
        };
        let key = BatchHmacKey {
            id: "k".into(),
            secret: vec![1u8; 32],
        };
        let limits = BatchLimits {
            max_items: 16,
            max_tokens: 200,
            max_scalars: 6_000,
            max_output_tokens: 4_096,
            system_prompt_token_estimate: 50,
        };
        let batches = pack_batches(
            std::slice::from_ref(&row),
            Uuid::nil(),
            &[0u8; 32],
            &key,
            &limits,
        );
        assert!(!batches.is_empty());
        let total_items: usize = batches.iter().map(|b| b.items.len()).sum();
        assert!(
            total_items > 1,
            "expected token-cap re-split into multiple items, got {total_items}"
        );
        let rejoined: String = batches
            .iter()
            .flat_map(|b| b.items.iter().map(|i| i.text.as_str()))
            .collect();
        let mini = minimize_text(&long);
        assert_eq!(
            rejoined.chars().filter(|c| !c.is_whitespace()).count(),
            mini.chars().filter(|c| !c.is_whitespace()).count(),
            "content scalars dropped during re-split"
        );
    }

    #[test]
    fn aggregate_picks_higher_risk() {
        let id = Uuid::from_u128(7);
        let segs = vec![
            SegmentResult {
                content_item_id: id,
                segment_ordinal: 0,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("negativity".into()),
                confidence: 0.9,
                reasons: vec![AggregateReason {
                    code: "sustained_hostility".into(),
                    summary: "hostile".into(),
                }],
                evidence: vec![AggregateEvidence {
                    text: "hate".into(),
                    supports_reason_code: "sustained_hostility".into(),
                }],
            },
            SegmentResult {
                content_item_id: id,
                segment_ordinal: 1,
                decision: "flag".into(),
                risk: "high".into(),
                category: Some("inappropriate_language".into()),
                confidence: 0.5,
                reasons: vec![AggregateReason {
                    code: "targeted_insult".into(),
                    summary: "insult".into(),
                }],
                evidence: vec![AggregateEvidence {
                    text: "idiot".into(),
                    supports_reason_code: "targeted_insult".into(),
                }],
            },
        ];
        let agg = aggregate_source_results(&segs);
        assert_eq!(agg.len(), 1);
        assert_eq!(agg[0].risk, "high");
        assert_eq!(agg[0].category.as_deref(), Some("inappropriate_language"));
        assert_eq!(agg[0].decision, "flag");
    }

    #[test]
    fn aggregate_equal_risk_higher_confidence_then_lower_ordinal() {
        let id = Uuid::from_u128(3);
        let mk = |ord: u32, conf: f64, summary: &str| SegmentResult {
            content_item_id: id,
            segment_ordinal: ord,
            decision: "flag".into(),
            risk: "low".into(),
            category: Some("public_image".into()),
            confidence: conf,
            reasons: vec![AggregateReason {
                code: "admitted_misconduct".into(),
                summary: summary.into(),
            }],
            evidence: vec![AggregateEvidence {
                text: summary.into(),
                supports_reason_code: "admitted_misconduct".into(),
            }],
        };
        let agg = aggregate_source_results(&[mk(0, 0.4, "a"), mk(1, 0.8, "b")]);
        assert_eq!(agg[0].confidence, 0.8);

        let agg2 = aggregate_source_results(&[mk(2, 0.5, "x"), mk(1, 0.5, "y")]);
        assert_eq!(agg2[0].confidence, 0.5);
        assert!(agg2[0].reasons.iter().any(|r| r.summary == "y"));
    }

    #[test]
    fn aggregate_caps_reasons_and_evidence_at_three() {
        let id = Uuid::from_u128(9);
        let mut reasons = Vec::new();
        let mut evidence = Vec::new();
        for i in 0..5 {
            reasons.push(AggregateReason {
                code: format!("code_{i}"),
                summary: format!("r{i}"),
            });
            evidence.push(AggregateEvidence {
                text: format!("e{i}"),
                supports_reason_code: format!("code_{i}"),
            });
        }
        let segs = vec![
            SegmentResult {
                content_item_id: id,
                segment_ordinal: 0,
                decision: "flag".into(),
                risk: "medium".into(),
                category: Some("inappropriate_language".into()),
                confidence: 0.7,
                reasons: reasons[..3].to_vec(),
                evidence: evidence[..3].to_vec(),
            },
            SegmentResult {
                content_item_id: id,
                segment_ordinal: 1,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("inappropriate_language".into()),
                confidence: 0.6,
                reasons: reasons[2..].to_vec(),
                evidence: evidence[2..].to_vec(),
            },
        ];
        let agg = aggregate_source_results(&segs);
        assert!(agg[0].reasons.len() <= 3);
        assert!(agg[0].evidence.len() <= 3);
        assert_eq!(agg[0].reasons[0].summary, "r0");
    }

    #[test]
    fn empty_after_minimize_omitted() {
        let row = NormalizedRow {
            content_item_id: Uuid::nil(),
            platform: "reddit".into(),
            kind: "post".into(),
            authorship: Authorship::Amplified,
            text: "   <br>  ".into(),
            content_hmac: vec![0u8; 32],
            source_logical_id: "e".into(),
            source_order: 0,
        };
        let key = BatchHmacKey {
            id: "k".into(),
            secret: vec![1u8; 32],
        };
        let batches = pack_batches(
            std::slice::from_ref(&row),
            Uuid::nil(),
            &[0u8; 32],
            &key,
            &BatchLimits::default(),
        );
        assert!(batches.is_empty());
    }

    #[test]
    fn authorship_storage_mapping() {
        assert_eq!(
            Authorship::from_storage("owner_authored"),
            Some(Authorship::Authored)
        );
        assert_eq!(
            Authorship::from_storage("reshared"),
            Some(Authorship::Amplified)
        );
        assert_eq!(Authorship::Authored.as_str(), "authored");
        assert_eq!(Authorship::Amplified.as_str(), "amplified");
    }
}
