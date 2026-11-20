use ghostpost_backend::import::{archive::SafeArchive, pipeline::{produce, ParsedItem}, ArchivePlatform, Authorship, FormatFamily, ImportError, NormalizedArchiveRecord, RecordType};
use std::io::{Cursor, Write};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, ZipWriter};

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(*name, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn parse(bytes: Vec<u8>, platform: ArchivePlatform, family: FormatFamily) -> Result<Vec<NormalizedArchiveRecord>, ImportError> {
    let archive = SafeArchive::index(Cursor::new(bytes), platform)?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let producer = std::thread::spawn(move || produce(archive, platform, Uuid::nil(), family, tx));
    let mut rows = Vec::new();
    while let Some(item) = rx.blocking_recv() {
        match item {
            ParsedItem::Instagram(record) | ParsedItem::TikTok(record) => rows.push(record),
            _ => panic!("wrong platform parser"),
        }
    }
    producer.join().unwrap()?;
    Ok(rows)
}

#[test]
fn public_records_flow_through_safe_archive_and_bounded_channel_without_private_text() {
    let cases: &[(ArchivePlatform, FormatFamily, &str, &[u8], &str)] = &[
        (ArchivePlatform::Instagram, FormatFamily::InstagramJson, "content/posts_1.json", br#"[{"title":"ghost_demo public caption","creation_timestamp":1682942400,"media":[{"uri":"media/demo.jpg"}]}]"#, "ghost_demo public caption"),
        (ArchivePlatform::TikTok, FormatFamily::TikTokJson, "user_data_tiktok.json", br#"{"Comment":{"Comments":{"CommentsList":[{"Date":"2023-05-01 12:00:00","Comment":"ghost_demo public comment"}]}},"Direct Messages":{"CommentsList":[{"Date":"2023-05-01 12:00:00","Comment":"PRIVATE MUST NOT IMPORT"}]}}"#, "ghost_demo public comment"),
        (ArchivePlatform::TikTok, FormatFamily::TikTokTxt, "Comments/Comments.txt", b"Date: 2023-05-01 12:00:00\nComment: ghost_demo public comment\nPhoto: None\nUrl: https://example.com/comment\n", "ghost_demo public comment"),
    ];
    for (platform, family, path, payload, expected) in cases {
        let bytes = zip(&[(path, payload), ("messages/inbox/ghost_demo/message_1.json", b"PRIVATE MUST NOT IMPORT")]);
        let rows = parse(bytes.clone(), *platform, *family).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].body.as_deref(), Some(*expected));
        assert_eq!(rows[0].platform, *platform);
        assert_eq!(rows[0].authorship, Authorship::OwnerAuthored);
        assert_eq!(rows[0].record_type, if *platform == ArchivePlatform::Instagram { RecordType::Post } else { RecordType::Comment });
        assert_eq!(rows[0].created_at.unwrap().timestamp(), 1682942400);
        assert_eq!(rows, parse(bytes, *platform, *family).unwrap(), "source identity must be deterministic");
    }
}

#[test]
fn empty_or_private_only_archives_never_succeed_as_public_imports() {
    for (platform, family, path, payload) in [
        (ArchivePlatform::Instagram, FormatFamily::InstagramJson, "content/posts_1.json", b"[]".as_slice()),
        (ArchivePlatform::TikTok, FormatFamily::TikTokJson, "user_data_tiktok.json", br#"{"Direct Messages":{"CommentsList":[{"Comment":"private"}]}}"#.as_slice()),
        (ArchivePlatform::TikTok, FormatFamily::TikTokTxt, "Comments/Comments.txt", b"".as_slice()),
    ] {
        assert_eq!(parse(zip(&[(path, payload)]), platform, family).unwrap_err().code, "no_supported_content");
    }
}

#[test]
fn every_pair_of_platform_signatures_is_ambiguous_and_wrong_selection_is_mismatch() {
    let signatures = [
        (ArchivePlatform::Reddit, "comments.csv"),
        (ArchivePlatform::X, "data/tweets.js"),
        (ArchivePlatform::Instagram, "content/posts_1.json"),
        (ArchivePlatform::TikTok, "user_data_tiktok.json"),
    ];
    for (i, (platform, path)) in signatures.iter().enumerate() {
        for (j, (other, other_path)) in signatures.iter().enumerate() {
            if i == j { continue; }
            let error = SafeArchive::index(Cursor::new(zip(&[(path, b"[]")])), *other).err().unwrap();
            assert_eq!(error.code, "platform_mismatch");
            let error = SafeArchive::index(Cursor::new(zip(&[(path, b"[]"), (other_path, b"[]")])), *platform).err().unwrap();
            assert_eq!(error.code, "ambiguous_archive");
        }
    }
}

#[test]
fn explicitly_nonpublic_tiktok_posts_are_excluded_in_json_and_txt() {
    let json = br#"{"Post":{"Posts":{"VideoList":[{"Date":"2023-05-01 12:00:00","Title":"public caption","Who can view":"Everyone"},{"Date":"2023-05-02 12:00:00","Title":"PRIVATE caption","Who can view":"Only me"},{"Date":"2023-05-03 12:00:00","Title":"friends caption","Who can view":"Friends"}]}}}"#;
    let txt = b"Date: 2023-05-01 12:00:00\nTitle: public caption\nWho can view: Everyone\n\nDate: 2023-05-02 12:00:00\nTitle: PRIVATE caption\nWho can view: Only me\n";
    for (path, bytes, family) in [
        ("user_data_tiktok.json", json.as_slice(), FormatFamily::TikTokJson),
        ("Posts/Posts.txt", txt.as_slice(), FormatFamily::TikTokTxt),
    ] {
        let records = parse(zip(&[(path, bytes)]), ArchivePlatform::TikTok, family).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].body.as_deref(), Some("public caption"));
    }
}
