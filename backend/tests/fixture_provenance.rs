use ghostpost_backend::auth::catalog::platforms_catalog;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceManifest {
    family: String,
    fixture: String,
    fixture_sha256: String,
    export_generation_date: String,
    observed_wrappers: Option<Vec<String>>,
    observed_headers: Option<Vec<String>>,
    redaction_transform: String,
    approved_for_production: bool,
    #[serde(default)]
    fixture_kind: Option<String>,
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// Tree hashes frame both paths and bytes, independent of traversal order or ZIP metadata.
fn tree_sha256(root: &Path) -> String {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("read fixture tree") {
            let path = entry.unwrap().path();
            assert!(!path.is_symlink(), "fixture trees cannot contain symlinks");
            if path.is_dir() { collect(root, &path, files); }
            else { files.insert(path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"), fs::read(path).unwrap()); }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    assert!(!files.is_empty(), "fixture tree must contain source files");
    let mut hash = Sha256::new();
    for (name, bytes) in files {
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

#[test]
fn advertised_archive_families_have_integrity_checked_honest_provenance() {
    let root = fixtures_root();
    let provenance_dir = root.join("provenance");
    let enabled: BTreeSet<&str> = platforms_catalog()
        .platforms
        .iter()
        .filter(|platform| platform.archive_enabled)
        .map(|platform| platform.id)
        .collect();
    assert_eq!(enabled, BTreeSet::from(["reddit", "x", "instagram", "tiktok"]));

    let entries = fs::read_dir(&provenance_dir).unwrap_or_else(|error| {
        panic!(
            "advertised archive ingestion requires provenance manifests at {}: {error}",
            provenance_dir.display()
        )
    });
    let mut approved_by_family = BTreeMap::<String, Vec<String>>::new();
    let mut saw_reddit_posts_gate = false;
    let mut synthetic_families = BTreeSet::new();

    for entry in entries {
        let path = entry.expect("read provenance entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let manifest_bytes = fs::read(&path).expect("read provenance manifest");
        let manifest: ProvenanceManifest = serde_json::from_slice(&manifest_bytes)
            .unwrap_or_else(|error| panic!("{} is not a valid provenance manifest: {error}", path.display()));

        assert!(
            matches!(manifest.family.as_str(), "RedditGdprCsv" | "XGdpr" | "XClassic" | "InstagramJson" | "TikTokJson" | "TikTokTxt"),
            "{} declares an unknown family {}",
            path.display(),
            manifest.family
        );
        assert!(
            manifest.export_generation_date.len() == 10
                && manifest.export_generation_date.as_bytes()[4] == b'-'
                && manifest.export_generation_date.as_bytes()[7] == b'-'
                && manifest
                    .export_generation_date
                    .bytes()
                    .enumerate()
                    .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()),
            "{} must pin an observed YYYY-MM-DD export generation date",
            path.display()
        );
        assert!(
            !manifest.redaction_transform.trim().is_empty(),
            "{} must document the irreversible redaction/pseudonymization transform",
            path.display()
        );
        let synthetic = manifest.fixture_kind.as_deref() == Some("synthetic_tree");
        if synthetic {
            assert!(!manifest.approved_for_production, "synthetic examples are not reviewed real exports");
            assert!(matches!(manifest.family.as_str(), "InstagramJson" | "TikTokJson" | "TikTokTxt"));
        } else {
            assert!(!manifest.fixture.contains("synthetic"), "synthetic examples cannot satisfy redacted-real gates");
        }
        assert!(
            !manifest.fixture.starts_with('/')
                && !manifest.fixture.split('/').any(|segment| segment == ".."),
            "fixture paths are relative to tests/fixtures and cannot escape it"
        );

        let fixture_path = root.join(&manifest.fixture);
        if synthetic {
            assert_eq!(manifest.fixture_sha256, tree_sha256(&fixture_path), "synthetic tree changed without updating its manifest: {}", fixture_path.display());
            synthetic_families.insert(manifest.family);
            continue;
        }
        if manifest.fixture.ends_with("reddit/user_redacted_posts_v1.zip")
            || manifest.fixture == "reddit/user_redacted_posts_v1.zip"
        {
            saw_reddit_posts_gate = true;
            assert!(
                !manifest.approved_for_production,
                "Reddit posts remain provisional until the separately reviewed production gate is intentionally changed"
            );
        }

        if manifest.approved_for_production {
            let fixture = fs::read(&fixture_path).unwrap_or_else(|error| {
                panic!(
                    "approved manifest {} references missing fixture {}: {error}",
                    path.display(),
                    fixture_path.display()
                )
            });
            assert_eq!(
                manifest.fixture_sha256,
                sha256_hex(&fixture),
                "approved fixture bytes changed without a provenance review: {}",
                fixture_path.display()
            );
            match manifest.family.as_str() {
                "RedditGdprCsv" => assert!(
                    manifest
                        .observed_headers
                        .as_ref()
                        .is_some_and(|headers| !headers.is_empty()),
                    "approved Reddit provenance records the observed exact CSV header"
                ),
                "XGdpr" | "XClassic" => assert!(
                    manifest
                        .observed_wrappers
                        .as_ref()
                        .is_some_and(|wrappers| !wrappers.is_empty()),
                    "approved X provenance records the observed JS wrapper"
                ),
                _ => unreachable!(),
            }
            approved_by_family
                .entry(manifest.family)
                .or_default()
                .push(manifest.fixture);
        }
    }

    for required_family in ["RedditGdprCsv", "XGdpr", "XClassic"] {
        assert!(
            approved_by_family.contains_key(required_family),
            "enabled production archive family {required_family} has no approved redacted-real fixture; synthetic fixtures cannot clear this gate"
        );
    }
    for required_family in ["InstagramJson", "TikTokJson", "TikTokTxt"] {
        assert!(synthetic_families.contains(required_family), "new family {required_family} requires an explicitly synthetic fixture tree");
    }
    assert!(
        saw_reddit_posts_gate,
        "Reddit posts require an explicit unapproved provenance marker until a reviewed redacted-real posts export lands"
    );
}
