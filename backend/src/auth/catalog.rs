//! Static platform / onboarding catalog aligned with app domain unions.

use serde::Serialize;

pub const CURRENT_DISCLOSURE_VERSION: &str = "2026-07-23";
pub const PLATFORMS_REVISION: &str = "2026-07-23";

pub const COMING_UP_IDS: &[&str] = &[
    "rush",
    "college_apps",
    "job_interviews",
    "friends_family",
    "just_concerned",
    "something_else",
];

pub const CONCERN_IDS: &[&str] = &[
    "inappropriate_language",
    "drinking_drugs",
    "political_takes",
    "controversial_topics",
    "negativity",
    "public_image",
    "other",
];

pub const ARCHIVE_ENABLED_PLATFORMS: &[&str] = &["reddit", "x"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformOption {
    pub id: &'static str,
    pub label: &'static str,
    pub glyph: &'static str,
    pub color: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_image: Option<bool>,
    pub archive_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LabeledOption {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformsCatalog {
    pub revision: &'static str,
    pub platforms: Vec<PlatformOption>,
    pub coming_up_options: Vec<LabeledOption>,
    pub concern_options: Vec<LabeledOption>,
}

pub fn platforms_catalog() -> PlatformsCatalog {
    PlatformsCatalog {
        revision: PLATFORMS_REVISION,
        platforms: vec![
            PlatformOption {
                id: "facebook",
                label: "Facebook",
                glyph: "f",
                color: "#0866FF",
                use_image: None,
                archive_enabled: false,
            },
            PlatformOption {
                id: "reddit",
                label: "Reddit",
                glyph: "●",
                color: "#FF4500",
                use_image: None,
                archive_enabled: true,
            },
            PlatformOption {
                id: "instagram",
                label: "Instagram",
                glyph: "ig",
                color: "#E1306C",
                use_image: Some(true),
                archive_enabled: false,
            },
            PlatformOption {
                id: "tiktok",
                label: "TikTok",
                glyph: "♪",
                color: "#111111",
                use_image: Some(true),
                archive_enabled: false,
            },
            PlatformOption {
                id: "x",
                label: "X",
                glyph: "𝕏",
                color: "#111111",
                use_image: Some(true),
                archive_enabled: true,
            },
        ],
        coming_up_options: vec![
            LabeledOption { id: "rush", label: "Rush" },
            LabeledOption { id: "college_apps", label: "College apps" },
            LabeledOption { id: "job_interviews", label: "Job interviews" },
            LabeledOption { id: "friends_family", label: "Friends or family" },
            LabeledOption { id: "just_concerned", label: "Just concerned" },
            LabeledOption { id: "something_else", label: "Something else" },
        ],
        concern_options: vec![
            LabeledOption { id: "inappropriate_language", label: "Inappropriate language" },
            LabeledOption { id: "drinking_drugs", label: "Drinking / drugs" },
            LabeledOption { id: "political_takes", label: "Political takes" },
            LabeledOption { id: "controversial_topics", label: "Controversial topics" },
            LabeledOption { id: "negativity", label: "Negativity" },
            LabeledOption { id: "public_image", label: "Public image" },
            LabeledOption { id: "other", label: "Other" },
        ],
    }
}

pub fn is_known_coming_up(id: &str) -> bool {
    COMING_UP_IDS.contains(&id)
}

pub fn is_known_concern(id: &str) -> bool {
    CONCERN_IDS.contains(&id)
}

pub fn is_archive_enabled_platform(id: &str) -> bool {
    ARCHIVE_ENABLED_PLATFORMS.contains(&id)
}

pub fn is_known_platform(id: &str) -> bool {
    matches!(id, "facebook" | "reddit" | "instagram" | "tiktok" | "x")
}
