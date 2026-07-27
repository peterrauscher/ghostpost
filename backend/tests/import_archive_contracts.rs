use ghostpost_backend::r#import::archive::SafeArchive;
use ghostpost_backend::r#import::reddit::parse_csv;
use ghostpost_backend::r#import::x::{parse_tweets, unwrap_js};
use ghostpost_backend::r#import::{
    ArchivePlatform, Authorship, ContentState, FormatConfidence, FormatFamily, ImportErrorCode,
    RecordType, RelationConfidence, TextFormat,
};
use std::io::{Cursor, Write};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Clone, Copy)]
struct ZipEntry<'a> {
    name: &'a str,
    bytes: &'a [u8],
    compression: CompressionMethod,
}

fn zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries {
        writer
            .start_file(
                entry.name,
                SimpleFileOptions::default().compression_method(entry.compression),
            )
            .expect("start synthetic ZIP entry");
        writer
            .write_all(entry.bytes)
            .expect("write synthetic ZIP entry");
    }
    writer.finish().expect("finish synthetic ZIP").into_inner()
}

fn stored(name: &str, bytes: &[u8]) -> Vec<u8> {
    zip(&[ZipEntry {
        name,
        bytes,
        compression: CompressionMethod::Stored,
    }])
}

fn error_code(result: Result<SafeArchive, ghostpost_backend::r#import::ImportError>) -> &'static str {
    match result {
        Ok(_) => panic!("archive must be rejected"),
        Err(error) => error.code,
    }
}

fn replace_all(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(needle.len(), replacement.len());
    let mut replaced = 0;
    let mut offset = 0;
    while let Some(relative) = bytes[offset..]
        .windows(needle.len())
        .position(|window| window == needle)
    {
        let start = offset + relative;
        bytes[start..start + needle.len()].copy_from_slice(replacement);
        replaced += 1;
        offset = start + needle.len();
    }
    assert!(replaced >= 2, "ZIP name should occur in local and central headers");
}

fn patch_zip_header_u16(bytes: &mut [u8], local_offset: usize, central_offset: usize, value: u16) {
    let mut local = 0;
    let mut central = 0;
    for index in 0..bytes.len().saturating_sub(4) {
        match &bytes[index..index + 4] {
            b"PK\x03\x04" => {
                bytes[index + local_offset..index + local_offset + 2]
                    .copy_from_slice(&value.to_le_bytes());
                local += 1;
            }
            b"PK\x01\x02" => {
                bytes[index + central_offset..index + central_offset + 2]
                    .copy_from_slice(&value.to_le_bytes());
                central += 1;
            }
            _ => {}
        }
    }
    assert!(local > 0 && central > 0, "must patch both ZIP header kinds");
}

fn set_zip_flag(bytes: &mut [u8], flag: u16) {
    for index in 0..bytes.len().saturating_sub(4) {
        let offset = match &bytes[index..index + 4] {
            b"PK\x03\x04" => Some(index + 6),
            b"PK\x01\x02" => Some(index + 8),
            _ => None,
        };
        if let Some(offset) = offset {
            let current = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            bytes[offset..offset + 2].copy_from_slice(&(current | flag).to_le_bytes());
        }
    }
}

#[test]
fn safe_zip_rejects_noncanonical_paths_before_any_content_is_read() {
    let overlong = format!("{}.csv", "a".repeat(509));
    let too_many_segments = "a/b/c/d/e/f/g/h/comments.csv";
    let cases = [
        ("absolute", "/comments.csv"),
        ("dotdot", "safe/../comments.csv"),
        ("dot", "safe/./comments.csv"),
        ("backslash", "safe\\comments.csv"),
        ("drive", "C:comments.csv"),
        ("empty segment", "safe//comments.csv"),
        ("too many segments", too_many_segments),
        ("overlong raw name", overlong.as_str()),
    ];

    for (name, path) in cases {
        let bytes = stored(path, b"not parsed");
        assert_eq!(
            error_code(SafeArchive::index(bytes, ArchivePlatform::Reddit)),
            "unsafe_zip_entry",
            "case {name}"
        );
    }

    let mut nul = stored("comments.csv", b"not parsed");
    replace_all(&mut nul, b"comments.csv", b"comments\0csv");
    assert_eq!(
        error_code(SafeArchive::index(nul, ArchivePlatform::Reddit)),
        "unsafe_zip_entry",
        "NUL in a raw entry name"
    );

    let mut invalid_utf8 = stored("comments.csv", b"not parsed");
    replace_all(&mut invalid_utf8, b"comments.csv", b"comment\xff.csv");
    set_zip_flag(&mut invalid_utf8, 1 << 11);
    assert_eq!(
        error_code(SafeArchive::index(invalid_utf8, ArchivePlatform::Reddit)),
        "unsafe_zip_entry",
        "entry names must be valid UTF-8"
    );
}

#[test]
fn safe_zip_rejects_duplicate_canonical_names() {
    let mut duplicate = zip(&[
        ZipEntry {
            name: "comments.csv",
            bytes: b"first",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "commentz.csv",
            bytes: b"second",
            compression: CompressionMethod::Stored,
        },
    ]);
    replace_all(&mut duplicate, b"commentz.csv", b"comments.csv");
    assert_eq!(
        error_code(SafeArchive::index(duplicate, ArchivePlatform::Reddit)),
        "duplicate_zip_entry"
    );
}

#[test]
fn safe_zip_rejects_symlinks_before_opening_them() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "comments.csv",
            "private/target.csv",
            SimpleFileOptions::default(),
        )
        .expect("synthetic symlink");
    let symlink = writer.finish().unwrap().into_inner();
    assert_eq!(
        error_code(SafeArchive::index(symlink, ArchivePlatform::Reddit)),
        "unsafe_zip_entry"
    );
}

#[test]
fn safe_zip_rejects_encrypted_selected_entries() {
    let mut encrypted = stored("comments.csv", b"ciphertext-shaped bytes");
    set_zip_flag(&mut encrypted, 1);
    assert_eq!(
        error_code(SafeArchive::index(encrypted, ArchivePlatform::Reddit)),
        "encrypted_zip"
    );
}

#[test]
fn safe_zip_rejects_nested_archives_even_when_not_allowlisted() {
    let nested = stored("nested.zip", b"PK\x03\x04synthetic nested archive");
    assert_eq!(
        error_code(SafeArchive::index(nested, ArchivePlatform::Reddit)),
        "unsafe_zip_entry"
    );
}

#[test]
fn safe_zip_rejects_unsupported_selected_entry_compression() {
    let mut unsupported = stored("comments.csv", b"unsupported compression payload");
    patch_zip_header_u16(&mut unsupported, 8, 10, 99);
    assert_eq!(
        error_code(SafeArchive::index(unsupported, ArchivePlatform::Reddit)),
        "unsupported_compression"
    );
}

#[test]
fn safe_zip_enforces_per_entry_and_aggregate_compression_ratio_before_inflate() {
    let compressible = vec![0_u8; 256 * 1024];
    let per_entry = zip(&[ZipEntry {
        name: "comments.csv",
        bytes: &compressible,
        compression: CompressionMethod::Deflated,
    }]);
    assert_eq!(
        error_code(SafeArchive::index(per_entry, ArchivePlatform::Reddit)),
        "zip_limit_exceeded"
    );

    let moderately_compressible = (0..400_000)
        .map(|index| b'a' + (index % 16) as u8)
        .collect::<Vec<_>>();
    let aggregate = zip(&[
        ZipEntry {
            name: "comments.csv",
            bytes: &moderately_compressible,
            compression: CompressionMethod::Deflated,
        },
        ZipEntry {
            name: "posts.csv",
            bytes: &moderately_compressible,
            compression: CompressionMethod::Deflated,
        },
    ]);
    assert_eq!(
        error_code(SafeArchive::index(aggregate, ArchivePlatform::Reddit)),
        "zip_limit_exceeded"
    );
}

#[test]
fn safe_zip_enforces_entry_count_and_selected_entry_count() {
    let mut selected_writer = ZipWriter::new(Cursor::new(Vec::new()));
    for part in 0..=4_096 {
        selected_writer
            .start_file(
                format!("data/tweets-part{part}.js"),
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
    }
    let selected = selected_writer.finish().unwrap().into_inner();
    assert_eq!(
        error_code(SafeArchive::index(selected, ArchivePlatform::X)),
        "zip_limit_exceeded"
    );

    let mut count_writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..=100_000 {
        count_writer
            .start_file(
                format!("excluded/{index:06}.txt"),
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
    }
    let too_many = count_writer.finish().unwrap().into_inner();
    assert_eq!(
        error_code(SafeArchive::index(too_many, ArchivePlatform::X)),
        "zip_limit_exceeded"
    );
}

#[test]
fn excluded_private_sources_are_never_opened_or_decompressed() {
    let valid = b"id,permalink,date,subreddit,parent,body\nc1,/r/synthetic/comments/t3_p/c1,2026-07-23T12:00:00Z,test,t3_p,kept\n";
    let mut archive = zip(&[
        ZipEntry {
            name: "comments.csv",
            bytes: valid,
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/direct-messages.js",
            bytes: b"private direct message bytes",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/tweets_media/private.jpg",
            bytes: b"private media bytes",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/like.js",
            bytes: b"private like bytes",
            compression: CompressionMethod::Stored,
        },
    ]);
    for payload in [
        b"private direct message bytes".as_slice(),
        b"private media bytes".as_slice(),
        b"private like bytes".as_slice(),
    ] {
        let offset = archive
            .windows(payload.len())
            .position(|window| window == payload)
            .expect("excluded payload bytes in ZIP");
        archive[offset] ^= 0xff;
    }
    let indexed = SafeArchive::index(archive, ArchivePlatform::Reddit)
        .expect("corrupt excluded payloads must not affect indexing");
    let opened = indexed
        .read_selected()
        .expect("corrupt excluded payloads must never be decompressed");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened.get("comments.csv").map(Vec::as_slice), Some(valid.as_slice()));

    let selected_only = stored("comments.csv", valid);
    let indexed = SafeArchive::index(selected_only, ArchivePlatform::Reddit)
        .expect("allowlisted comments entry");
    let opened = indexed.read_selected().expect("read selected entry");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened.get("comments.csv").map(Vec::as_slice), Some(valid.as_slice()));
}

#[test]
fn x_allowlist_accepts_only_exact_modern_shards_and_classic_months() {
    for path in [
        "data/tweets.js",
        "data/tweet.js",
        "data/tweets-part0.js",
        "data/tweet-part0001.js",
        "data/tweets/2026_01.js",
    ] {
        SafeArchive::index(stored(path, b"[]"), ArchivePlatform::X)
            .unwrap_or_else(|error| panic!("{path} should be allowlisted: {}", error.code));
    }

    for path in [
        "data/tweets-part.js",
        "data/tweets-part12345.js",

        "data/tweets-partx.js",
        "data/tweet-part-1.js",
        "data/tweets/2026_00.js",
        "data/tweets/2026_13.js",
        "data/tweets/anything.js",
        "data/js/tweet_index.js",
        "data/direct-messages.js",
        "data/tweets_media/1.jpg",
        "data/like.js",
    ] {
        assert_eq!(
            error_code(SafeArchive::index(stored(path, b"[]"), ArchivePlatform::X)),
            "no_supported_content",
            "{path} must not be opened"
        );
    }
}

#[test]
fn x_shards_are_validated_contiguous_and_selected_in_numeric_order() {
    let out_of_order = zip(&[
        ZipEntry {
            name: "data/tweets-part2.js",
            bytes: b"[]",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/tweets-part0.js",
            bytes: b"[]",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/tweets-part1.js",
            bytes: b"[]",
            compression: CompressionMethod::Stored,
        },
    ]);
    let indexed = SafeArchive::index(out_of_order, ArchivePlatform::X)
        .expect("contiguous parts are accepted regardless of ZIP order");
    let names: Vec<&str> = indexed
        .selected
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "data/tweets-part0.js",
            "data/tweets-part1.js",
            "data/tweets-part2.js"
        ],
        "workers parse shards by numeric part, never central-directory order"
    );

    let gap = zip(&[
        ZipEntry {
            name: "data/tweets-part0.js",
            bytes: b"[]",
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/tweets-part2.js",
            bytes: b"[]",
            compression: CompressionMethod::Stored,
        },
    ]);
    assert_eq!(
        error_code(SafeArchive::index(gap, ArchivePlatform::X)),
        "unsupported_format_version"
    );
}

#[test]
fn zip_magic_rejects_sfx_trailing_payload_and_non_zip_bytes() {
    assert_eq!(
        error_code(SafeArchive::index(b"not a zip".to_vec(), ArchivePlatform::Reddit)),
        "not_zip"
    );

    let mut sfx = b"MZ synthetic preamble".to_vec();
    sfx.extend(stored("comments.csv", b"content"));
    assert_eq!(
        error_code(SafeArchive::index(sfx, ArchivePlatform::Reddit)),
        "not_zip"
    );

    let mut trailing = stored("comments.csv", b"content");
    trailing.extend_from_slice(b"adversarial trailing payload");
    assert_eq!(
        error_code(SafeArchive::index(trailing, ArchivePlatform::Reddit)),
        "unsupported_zip"
    );
}

fn parse_reddit(csv: &[u8], file: &str) -> Result<Vec<ghostpost_backend::r#import::NormalizedArchiveRecord>, ghostpost_backend::r#import::ImportError> {
    parse_csv(Cursor::new(csv), file, Uuid::nil())
}

#[test]
fn reddit_comments_support_bom_rfc4180_unicode_and_both_date_formats() {
    let csv = concat!(
        "\u{feff}id,permalink,date,subreddit,parent,body,ip,gildings\r\n",
        "c1,/r/synthetic/comments/t3_p/c1,2026-07-23T12:00:00-04:00,synthetic,t3_p,\"first line\r\nsecond, \"\"quoted\"\" cafe\u{301}\",192.0.2.1,99\r\n",
        "c2,/r/synthetic/comments/t1_c1/c2,2026-07-23 16:00:01 UTC,synthetic,t1_c1,=1+1,192.0.2.2,0\r\n"
    );
    let records = parse_reddit(csv.as_bytes(), "comments.csv").expect("valid comments export");
    assert_eq!(records.len(), 2);

    let first = &records[0];
    assert_eq!(first.schema_version, 1);
    assert_eq!(first.import_id, Uuid::nil());
    assert_eq!(first.platform, ArchivePlatform::Reddit);
    assert_eq!(first.source_logical_id, "c1");
    assert_eq!(first.source_revision_id, "c1");
    assert_eq!(first.record_type, RecordType::Comment);
    assert_eq!(first.text_format, TextFormat::Standard);
    assert_eq!(first.authorship, Authorship::OwnerAuthored);
    assert_eq!(first.state, ContentState::Active);
    assert_eq!(
        first.created_at.unwrap().to_rfc3339(),
        "2026-07-23T16:00:00+00:00"
    );
    assert_eq!(first.title, None);
    assert_eq!(first.body.as_deref(), Some("first line\nsecond, \"quoted\" café"));
    assert_eq!(first.parent_source_id.as_deref(), Some("p"));
    assert_eq!(first.quoted_source_id, None);
    assert_eq!(first.relation_confidence, RelationConfidence::Explicit);
    assert!(!first.text_truncated);
    assert_eq!(first.provenance.source_file, "comments.csv");
    assert_eq!(first.provenance.source_ordinal, 1);
    assert_eq!(first.provenance.format_family, FormatFamily::RedditGdprCsv);
    assert_eq!(first.provenance.format_confidence, FormatConfidence::Compatible);

    assert_eq!(records[1].body.as_deref(), Some("=1+1"), "CSV cells are data, never formulas");
    assert_eq!(records[1].parent_source_id.as_deref(), Some("c1"));
    assert_eq!(records[1].created_at.unwrap().timestamp(), 1_784_822_401);
}

#[test]
fn reddit_exact_comments_header_is_confirmed_but_posts_remain_provisional() {
    let comments = parse_reddit(
        b"id,permalink,date,subreddit,parent,body\nc1,/r/s/comments/t3_p/c1,2026-07-23T12:00:00Z,s,t3_p,body\n",
        "comments.csv",
    )
    .unwrap();
    assert_eq!(comments[0].provenance.format_confidence, FormatConfidence::Confirmed);

    let posts = parse_reddit(
        b"id,permalink,date,subreddit,title,url,body\np1,/r/s/comments/p1/title,2026-07-23T12:00:00Z,s,Title,https://example.invalid,Body\n",
        "posts.csv",
    )
    .unwrap();
    assert_eq!(posts[0].record_type, RecordType::Post);
    assert_eq!(posts[0].title.as_deref(), Some("Title"));
    assert_eq!(posts[0].body.as_deref(), Some("Body"));
    assert_eq!(posts[0].provenance.format_confidence, FormatConfidence::Provisional);
}

#[test]
fn reddit_sentinels_produce_bodyless_tombstones() {
    let posts = parse_reddit(
        concat!(
            "id,permalink,date,subreddit,title,url,body\n",
            "p1,/r/s/comments/p1/title,2026-07-23T12:00:00Z,s,[deleted by user],https://example.invalid,old body\n",
            "p2,/r/s/comments/p2/title,2026-07-23T12:00:00Z,s,old title,https://example.invalid,[removed]\n"
        )
        .as_bytes(),
        "posts.csv",
    )
    .expect("sentinel rows are valid tombstones");
    assert_eq!(posts[0].state, ContentState::Deleted);
    assert_eq!(posts[0].title, None);
    assert_eq!(posts[0].body, None);
    assert_eq!(posts[1].state, ContentState::Removed);
    assert_eq!(posts[1].title, None);
    assert_eq!(posts[1].body, None);
}

#[test]
fn reddit_rejects_schema_identity_parent_permalink_date_and_malformed_csv_boundaries() {
    let cases: &[(&str, &str, &[u8], &str)] = &[
        (
            "missing header",
            "comments.csv",
            b"id,permalink,date,subreddit,body\nc1,p,2026-07-23T12:00:00Z,s,body\n",
            "unsupported_format_version",
        ),
        (
            "uppercase id",
            "comments.csv",
            b"id,permalink,date,subreddit,parent,body\nC1,p,2026-07-23T12:00:00Z,s,t3_p,body\n",
            "invalid_record",
        ),
        (
            "invalid parent",
            "comments.csv",
            b"id,permalink,date,subreddit,parent,body\nc1,p,2026-07-23T12:00:00Z,s,t9_p,body\n",
            "invalid_record",
        ),
        (
            "untrusted date",
            "comments.csv",
            b"id,permalink,date,subreddit,parent,body\nc1,p,07/23/2026,s,t3_p,body\n",
            "invalid_record",
        ),
        (
            "empty active text",
            "comments.csv",
            b"id,permalink,date,subreddit,parent,body\nc1,p,2026-07-23T12:00:00Z,s,t3_p,   \n",
            "invalid_record",
        ),
        (
            "post permalink id mismatch",
            "posts.csv",
            b"id,permalink,date,subreddit,title,url,body\np1,/r/s/comments/p2/title,2026-07-23T12:00:00Z,s,title,u,body\n",
            "invalid_record",
        ),
        (
            "malformed selected CSV",
            "comments.csv",
            b"id,permalink,date,subreddit,parent,body\nc1,p,2026-07-23T12:00:00Z,s,t3_p,\"unterminated\n",
            "invalid_record",
        ),
    ];

    for (name, file, csv, expected) in cases {
        let error = parse_reddit(csv, file).expect_err(name);
        assert_eq!(error.code, *expected, "case {name}");
        assert!(!error.to_string().contains("unterminated"), "errors must not echo archive text");
    }
}

fn parse_x(input: &[u8]) -> Result<Vec<ghostpost_backend::r#import::NormalizedArchiveRecord>, ghostpost_backend::r#import::ImportError> {
    parse_tweets(input, Uuid::nil(), "data/tweets.js", FormatFamily::XGdpr)
}

#[test]
fn x_accepts_modern_and_classic_wrappers_bare_or_enveloped_tweets_and_exact_ids_dates() {
    let input = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"18446744073709551615","id":18446744073709551615,"full_text":"modern","created_at":"Thu Jul 23 12:00:00 +0000 2026"}},
      {"id":42,"text":"classic","created_at":"Thu Jul 23 12:00:01 +0000 2026"}
    ]"#;
    let rows = parse_x(input).expect("known JS wrapper and envelopes");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].source_logical_id, "18446744073709551615");
    assert_eq!(rows[0].source_revision_id, "18446744073709551615");
    assert_eq!(rows[0].body.as_deref(), Some("modern"));
    assert_eq!(rows[0].created_at.unwrap().timestamp(), 1_784_808_000);
    assert_eq!(rows[1].source_logical_id, "42");
    assert_eq!(rows[1].body.as_deref(), Some("classic"));

    let singular = String::from_utf8(input.to_vec())
        .unwrap()
        .replacen("window.YTD.tweets.part0", "window.YTD.tweet.part0", 1);
    assert_eq!(parse_x(singular.as_bytes()).unwrap().len(), 2);
}

#[test]
fn x_rejects_unknown_wrappers_suffix_code_malformed_json_and_inexact_ids() {
    let valid_tweet = r#"[{"tweet":{"id_str":"1","full_text":"body","created_at":"Thu Jul 23 12:00:00 +0000 2026"}}]"#;
    let cases = [
        (
            "unknown wrapper",
            format!("window.YTD.likes.part0 = {valid_tweet}"),
            "unsupported_format_version",
        ),
        (
            "missing part",
            format!("window.YTD.tweets = {valid_tweet}"),
            "unsupported_format_version",
        ),
        (
            "executable suffix",
            format!("window.YTD.tweets.part0 = {valid_tweet}; alert('suffix')"),
            "unsupported_format_version",
        ),
        (
            "malformed JSON",
            "window.YTD.tweets.part0 = [{]".into(),
            "invalid_schema",
        ),
        (
            "id disagreement",
            "window.YTD.tweets.part0 = [{\"tweet\":{\"id_str\":\"42\",\"id\":43,\"full_text\":\"body\",\"created_at\":\"Thu Jul 23 12:00:00 +0000 2026\"}}]".into(),
            "invalid_record",
        ),
        (
            "float id",
            "window.YTD.tweets.part0 = [{\"tweet\":{\"id\":42.5,\"full_text\":\"body\",\"created_at\":\"Thu Jul 23 12:00:00 +0000 2026\"}}]".into(),
            "invalid_record",
        ),
    ];

    for (name, input, expected) in cases {
        assert_eq!(parse_x(input.as_bytes()).expect_err(name).code, expected, "case {name}");
    }

    let oversized_prefix = format!("{}window.YTD.tweets.part0 = {valid_tweet}", " ".repeat(4_097));
    assert_eq!(
        unwrap_js(oversized_prefix.as_bytes()).expect_err("wrapper after prefix budget").code,
        "unsupported_format_version"
    );
}

#[test]
fn x_classification_priority_and_text_normalization_are_canonical() {
    let input = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"1","full_text":"RT @synthetic: reshared\r\ntext","retweeted":false,"in_reply_to_status_id_str":"99","created_at":"Thu Jul 23 12:00:00 +0000 2026"}},
      {"tweet":{"id_str":"2","full_text":"reply","in_reply_to_status_id_str":"1","created_at":"Thu Jul 23 12:00:01 +0000 2026"}},
      {"tweet":{"id_str":"3","full_text":"thought https://t.co/quote","created_at":"Thu Jul 23 12:00:02 +0000 2026","entities":{"urls":[{"url":"https://t.co/quote","expanded_url":"https://x.com/synthetic/status/2"}]}}},
      {"tweet":{"id_str":"4","full_text":"caf&eacute; &amp; cafe\u0301","created_at":"Thu Jul 23 12:00:03 +0000 2026"}}
    ]"#;
    let rows = parse_x(input).expect("classification fixture");

    assert_eq!(rows[0].record_type, RecordType::Repost, "repost wins over reply");
    assert_eq!(rows[0].authorship, Authorship::Reshared);
    assert_eq!(rows[0].body.as_deref(), Some("reshared\ntext"));
    assert_eq!(rows[0].parent_source_id, None);

    assert_eq!(rows[1].record_type, RecordType::Reply);
    assert_eq!(rows[1].parent_source_id.as_deref(), Some("1"));
    assert_eq!(rows[1].relation_confidence, RelationConfidence::Explicit);

    assert_eq!(rows[2].record_type, RecordType::Quote);
    assert_eq!(rows[2].body.as_deref(), Some("thought"));

    assert_eq!(rows[2].quoted_source_id.as_deref(), Some("2"));
    assert_eq!(rows[2].relation_confidence, RelationConfidence::Heuristic);

    assert_eq!(rows[3].record_type, RecordType::Post);
    assert_eq!(rows[3].body.as_deref(), Some("café & café"));
}

#[test]
fn x_quote_requires_exactly_one_external_status_and_removes_only_its_token() {
    let single = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"3","full_text":"thought https://t.co/quote keep https://example.invalid","created_at":"Thu Jul 23 12:00:02 +0000 2026","entities":{"urls":[{"url":"https://t.co/quote","expanded_url":"https://x.com/synthetic/status/2"},{"url":"https://example.invalid","expanded_url":"https://example.invalid"}]}}}
    ]"#;
    let quote = parse_x(single).expect("single external status quote");
    assert_eq!(quote[0].record_type, RecordType::Quote);
    assert_eq!(quote[0].quoted_source_id.as_deref(), Some("2"));
    assert_eq!(quote[0].relation_confidence, RelationConfidence::Heuristic);
    assert_eq!(
        quote[0].body.as_deref(),
        Some("thought keep https://example.invalid")
    );

    let ambiguous = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"4","full_text":"two https://t.co/a https://t.co/b","created_at":"Thu Jul 23 12:00:03 +0000 2026","entities":{"urls":[{"url":"https://t.co/a","expanded_url":"https://x.com/synthetic/status/1"},{"url":"https://t.co/b","expanded_url":"https://twitter.com/synthetic/status/2"}]}}}
    ]"#;
    let non_quote = parse_x(ambiguous).expect("ambiguous quote evidence is not guessed");
    assert_eq!(non_quote[0].record_type, RecordType::Post);
    assert_eq!(non_quote[0].quoted_source_id, None);
    assert_eq!(
        non_quote[0].body.as_deref(),
        Some("two https://t.co/a https://t.co/b")
    );
}

#[test]
fn x_edit_chains_select_latest_revision_and_require_complete_chain() {
    let complete = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"10","full_text":"old","created_at":"Thu Jul 23 12:00:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11"]}}},
      {"tweet":{"id_str":"11","full_text":"new","created_at":"Thu Jul 23 12:01:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11"]}}}
    ]"#;
    let records = parse_x(complete).expect("complete edit chain");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source_logical_id, "10");
    assert_eq!(records[0].source_revision_id, "11");
    assert_eq!(records[0].body.as_deref(), Some("new"));

    let incomplete = br#"window.YTD.tweets.part0 = [
      {"tweet":{"id_str":"10","full_text":"old","created_at":"Thu Jul 23 12:00:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11"]}}}
    ]"#;
    assert_eq!(parse_x(incomplete).expect_err("missing greatest revision").code, "invalid_edit_chain");
}

#[test]
fn x_deleted_tweets_are_bodyless_tombstones_and_absence_is_not_deletion() {
    let deleted = br#"window.YTD.deleted_tweets.part0 = [
      {"tweet":{"id_str":"51","created_at":"Thu Jul 23 12:00:00 +0000 2026"}}
    ]"#;
    let rows = parse_tweets(
        deleted,
        Uuid::nil(),
        "data/deleted-tweets.js",
        FormatFamily::XGdpr,
    )
    .expect("explicit deleted-tweets rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ContentState::Deleted);
    assert_eq!(rows[0].body, None);
    assert_eq!(rows[0].title, None);
}

#[test]
fn runtime_security_limits_match_the_cross_layer_plan_contract() {
    use ghostpost_backend::r#import::limits::*;

    assert_eq!(MAX_ARCHIVE_BYTES, 2_147_483_648);
    assert_eq!(MAX_ZIP_ENTRIES, 100_000);
    assert_eq!(MAX_ENTRY_NAME_BYTES, 512);
    assert_eq!(MAX_PATH_SEGMENTS, 8);
    assert_eq!(MAX_SELECTED_ENTRIES, 4_096);
    assert_eq!(MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES, 134_217_728);
    assert_eq!(MAX_SELECTED_TOTAL_UNCOMPRESSED_BYTES, 536_870_912);
    assert_eq!(MAX_SELECTED_ENTRY_RATIO, 200);
    assert_eq!(MAX_SELECTED_AGGREGATE_RATIO, 100);
    assert_eq!(MAX_CSV_HEADER_BYTES, 16_384);
    assert_eq!(MAX_CSV_RECORD_BYTES, 2_097_152);
    assert_eq!(MAX_TEXT_FIELD_BYTES, 1_048_576);
    assert_eq!(MAX_JSON_NESTING, 64);
    assert_eq!(MAX_NORMALIZED_RECORDS, 2_000_000);
    assert_eq!(MAX_JS_PREFIX_BYTES, 4_096);
    assert_eq!(MAX_IN_MEMORY_INDEX_BYTES, 32_777_216);
    assert_eq!(MAX_DISK_INDEX_BYTES, 268_435_456);
    assert_eq!(WORKER_IMPORT_MEMORY_BUDGET_BYTES, 805_306_368);
}

#[test]
fn reddit_enforces_header_record_and_text_byte_limits_before_normalization() {
    use ghostpost_backend::r#import::limits::{
        MAX_CSV_HEADER_BYTES, MAX_CSV_RECORD_BYTES, MAX_TEXT_FIELD_BYTES,
    };

    let oversized_header = format!(
        "id,permalink,date,subreddit,parent,body,{}\n",
        "x".repeat(MAX_CSV_HEADER_BYTES)
    );
    assert_eq!(
        parse_reddit(oversized_header.as_bytes(), "comments.csv")
            .expect_err("oversized CSV header")
            .code,
        "resource_limit"
    );

    let oversized_record = format!(
        "id,permalink,date,subreddit,parent,body,discarded\nc1,p,2026-07-23T12:00:00Z,s,t3_p,body,{}\n",
        "x".repeat(MAX_CSV_RECORD_BYTES)
    );
    assert_eq!(
        parse_reddit(oversized_record.as_bytes(), "comments.csv")
            .expect_err("oversized RFC4180 record including discarded cells")
            .code,
        "resource_limit"
    );

    let oversized_text = format!(
        "id,permalink,date,subreddit,parent,body\nc1,p,2026-07-23T12:00:00Z,s,t3_p,{}\n",
        "é".repeat(MAX_TEXT_FIELD_BYTES / 2 + 1)
    );
    assert_eq!(
        parse_reddit(oversized_text.as_bytes(), "comments.csv")
            .expect_err("text is limited in UTF-8 bytes, not scalar count")
            .code,
        "resource_limit"
    );
}

#[test]
fn x_rejects_json_beyond_the_nesting_budget() {
    use ghostpost_backend::r#import::limits::MAX_JSON_NESTING;

    let mut nested = "null".to_string();
    for _ in 0..=MAX_JSON_NESTING {
        nested = format!("{{\"nested\":{nested}}}");
    }
    let input = format!(
        "window.YTD.tweets.part0 = [{{\"tweet\":{{\"id_str\":\"1\",\"full_text\":\"body\",\"created_at\":\"Thu Jul 23 12:00:00 +0000 2026\",\"extra\":{nested}}}}}]"
    );
    assert_eq!(
        parse_x(input.as_bytes())
            .expect_err("JSON nesting over the limit")
            .code,
        "resource_limit"
    );
}

#[test]
fn every_terminal_archive_rejection_has_the_stable_public_code() {
    let cases = [
        (ImportErrorCode::UploadTooLarge, "upload_too_large"),
        (ImportErrorCode::NotZip, "not_zip"),
        (ImportErrorCode::UnsupportedZip, "unsupported_zip"),
        (ImportErrorCode::EncryptedZip, "encrypted_zip"),
        (ImportErrorCode::UnsafeZipEntry, "unsafe_zip_entry"),
        (ImportErrorCode::DuplicateZipEntry, "duplicate_zip_entry"),
        (ImportErrorCode::ZipLimitExceeded, "zip_limit_exceeded"),
        (
            ImportErrorCode::UnsupportedCompression,
            "unsupported_compression",
        ),
        (ImportErrorCode::IntegrityMismatch, "integrity_mismatch"),
        (ImportErrorCode::NoSupportedContent, "no_supported_content"),
        (ImportErrorCode::AmbiguousArchive, "ambiguous_archive"),
        (ImportErrorCode::PlatformMismatch, "platform_mismatch"),
        (
            ImportErrorCode::UnsupportedFormatVersion,
            "unsupported_format_version",
        ),
        (ImportErrorCode::InvalidEncoding, "invalid_encoding"),
        (ImportErrorCode::InvalidSchema, "invalid_schema"),
        (ImportErrorCode::InvalidRecord, "invalid_record"),
        (ImportErrorCode::DuplicateConflict, "duplicate_conflict"),
        (ImportErrorCode::InvalidEditChain, "invalid_edit_chain"),
        (ImportErrorCode::AmbiguousNoteJoin, "ambiguous_note_join"),
        (
            ImportErrorCode::AmbiguousRevisionOrder,
            "ambiguous_revision_order",
        ),
        (ImportErrorCode::ResourceLimit, "resource_limit"),
    ];

    for (error, expected) in cases {
        assert_eq!(error.as_str(), expected);
        let surfaced = ghostpost_backend::r#import::ImportError::from(error);
        assert_eq!(surfaced.code, expected);
        assert_eq!(surfaced.to_string(), format!("archive rejected: {expected}"));
    }
}

#[test]
fn archive_format_detection_fails_closed_on_ambiguous_or_declared_platform_mismatch() {
    let reddit = b"id,permalink,date,subreddit,parent,body\nc1,p,2026-07-23T12:00:00Z,s,t3_p,body\n";
    let x = br#"window.YTD.tweets.part0 = [{"tweet":{"id_str":"1","full_text":"body","created_at":"Thu Jul 23 12:00:00 +0000 2026"}}]"#;
    let ambiguous = zip(&[
        ZipEntry {
            name: "comments.csv",
            bytes: reddit,
            compression: CompressionMethod::Stored,
        },
        ZipEntry {
            name: "data/tweets.js",
            bytes: x,
            compression: CompressionMethod::Stored,
        },
    ]);
    assert_eq!(
        error_code(SafeArchive::index(ambiguous, ArchivePlatform::Reddit)),
        "ambiguous_archive"
    );

    let mismatch = stored("data/tweets.js", x);
    assert_eq!(
        error_code(SafeArchive::index(mismatch, ArchivePlatform::Reddit)),
        "platform_mismatch"
    );
}
