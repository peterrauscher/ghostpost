pub const MAX_ARCHIVE_BYTES:u64=2_147_483_648;pub const MAX_ZIP_ENTRIES:usize=100_000;pub const MAX_ENTRY_NAME_BYTES:usize=512;pub const MAX_PATH_SEGMENTS:usize=8;pub const MAX_SELECTED_ENTRIES:usize=4_096;pub const MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES:u64=134_217_728;pub const MAX_SELECTED_TOTAL_UNCOMPRESSED_BYTES:u64=536_870_912;pub const MAX_SELECTED_ENTRY_RATIO:u64=200;pub const MAX_SELECTED_AGGREGATE_RATIO:u64=100;pub const MAX_CSV_HEADER_BYTES:usize=16_384;pub const MAX_CSV_RECORD_BYTES:usize=2_097_152;pub const MAX_TEXT_FIELD_BYTES:usize=1_048_576;pub const MAX_JSON_NESTING:usize=64;pub const MAX_NORMALIZED_RECORDS:usize=2_000_000;pub const MAX_JS_PREFIX_BYTES:usize=4_096;
/// One top-level element of an X `.js` array (a tweet or note envelope) is
/// buffered at a time; anything larger is rejected rather than held.
pub const MAX_JSON_ELEMENT_BYTES:usize=4_194_304;
/// Per cross-record index (notes, revisions, edit chains); beyond this an index
/// spills to `import_index_entries` in Postgres.
pub const MAX_IN_MEMORY_INDEX_BYTES:usize=8_388_608;
/// Per index on disk. Sized to the selected-uncompressed cap with headroom.
pub const MAX_DISK_INDEX_BYTES:usize=1_073_741_824;
pub const WORKER_IMPORT_MEMORY_BUDGET_BYTES:usize=805_306_368;
/// Chunk size for spooling the pinned archive from object storage.
pub const SPOOL_CHUNK_BYTES:u64=8_388_608;
/// Parsed records in flight between the blocking parser and the async stager.
pub const PARSE_CHANNEL_CAPACITY:usize=256;
