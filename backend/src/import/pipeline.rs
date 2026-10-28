//! Blocking half of the import pipeline: reads selected ZIP entries one at a
//! time and sends parsed items over a bounded channel to the async stager.
//! Backpressure from `PARSE_CHANNEL_CAPACITY` keeps at most that many records
//! in flight, however large the archive.
use super::archive::SafeArchive;
use super::x::{self, XItem};
use super::{reddit, ArchivePlatform, FormatFamily, ImportError, ImportErrorCode, NormalizedArchiveRecord};
use std::io::{Read, Seek};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

#[derive(Debug)]
pub enum ParsedItem {
    Reddit(NormalizedArchiveRecord),
    X(XItem),
}

/// The consumer stopped (it failed or was cancelled); its error wins.
pub const CONSUMER_GONE: &str = "import consumer stopped";

fn send(tx: &Sender<ParsedItem>, item: ParsedItem) -> Result<(), ImportError> {
    tx.blocking_send(item).map_err(|_| ImportError { code: CONSUMER_GONE })
}

/// X sources in processing order: every `note-tweet.js` first (previews join
/// against notes), then tweet files in the archive's selected order.
pub fn x_sources(selected: &[String]) -> Vec<String> {
    let notes = selected.iter().filter(|name| name.ends_with("note-tweet.js"));
    let tweets = selected.iter().filter(|name| !name.ends_with("note-tweet.js") && !name.ends_with("manifest.js"));
    notes.chain(tweets).cloned().collect()
}

pub fn produce<R: Read + Seek>(
    mut archive: SafeArchive<R>,
    platform: ArchivePlatform,
    import_id: Uuid,
    family: FormatFamily,
    tx: Sender<ParsedItem>,
) -> Result<(), ImportError> {
    let names: Vec<String> = archive.selected.iter().map(|entry| entry.name.clone()).collect();
    match platform {
        ArchivePlatform::Reddit => {
            for name in names.iter().filter(|name| matches!(name.as_str(), "comments.csv" | "posts.csv")) {
                archive.read_entry(name, |reader| {
                    reddit::parse_csv(reader, name, import_id, &mut |record| send(&tx, ParsedItem::Reddit(record)))
                })?;
            }
        }
        ArchivePlatform::X => {
            for name in x_sources(&names) {
                archive.read_entry(&name, |reader| {
                    x::stream_file(reader, import_id, &name, family, &mut |item| send(&tx, ParsedItem::X(item)))
                })?;
            }
        }
    }
    if names.is_empty() { return Err(ImportErrorCode::NoSupportedContent.into()); }
    Ok(())
}
