//! Synthetic source trees become ZIPs only at runtime, with no directory entries.
use std::{collections::BTreeMap, fs, io::{Cursor, Write}, path::Path};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub fn fixture_zip(relative: &str) -> Vec<u8> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("fixture directory") {
            let path = entry.unwrap().path();
            assert!(!path.is_symlink());
            if path.is_dir() { collect(root, &path, files); }
            else { files.insert(path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"), fs::read(path).unwrap()); }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(relative);
    let mut files = BTreeMap::new();
    collect(&root, &root, &mut files);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
