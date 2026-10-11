//! The last-resort interface font: one face already installed on this machine.
//!
//! The embedded faces (Inter, egui's defaults, craft-fonts) come first in every family; this one
//! only draws characters none of them has, such as an Arabic file name in a build without
//! craft-fonts, or a Chinese interface in a build without an embedded Chinese face (#882). It is
//! read at runtime and never embedded or shipped (AGENTS.md §1.4), and `PDFCRAFT_SYSTEM_FONTS=0`
//! turns it off (published screenshots do). Only one face is ever installed: the one for what the
//! interface needs ([`need_for`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::FontData;
use pdfcraft_fonts::CjkPreference;

/// Larger files are not read: a font path is still untrusted input.
const MAX_BYTES: u64 = 32 << 20;
/// Chinese system faces are large collections (PingFang, Microsoft YaHei): a higher bound for them.
const MAX_HAN_BYTES: u64 = 96 << 20;
/// Faces tried in a collection (`.ttc`).
const MAX_FACES: u32 = 16;
/// Arabic letter alef: the face must have it to be worth loading.
const PROBE: char = '\u{0627}';

/// What the installed face is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Scripts no embedded face draws in a build without craft-fonts: Arabic and Hebrew file
    /// names, Latin and Cyrillic beyond Inter (the historical fallback).
    Broad,
    /// A Simplified Chinese interface in a build without an embedded `Hans` face.
    Simplified,
    /// A Traditional Chinese interface in a build without an embedded `Hant` face.
    Traditional,
}

impl Need {
    /// A character the face must have to be worth loading: one only the script it is for has.
    fn probe(self) -> char {
        match self {
            Self::Broad => PROBE,
            // 这 and 這: Simplified and Traditional forms Japanese faces and Segoe UI lack.
            Self::Simplified => '\u{8FD9}',
            Self::Traditional => '\u{9019}',
        }
    }

    fn max_bytes(self) -> u64 {
        if self == Self::Broad { MAX_BYTES } else { MAX_HAN_BYTES }
    }
}

/// What the installed face is needed for: a Chinese face when the interface is in Chinese and the
/// build embeds no face for that script, otherwise the broad fallback. Releases embed a Chinese
/// face, so they keep the broad one.
pub fn need_for(cjk: CjkPreference, has_hans: bool, has_hant: bool) -> Need {
    match cjk {
        CjkPreference::Simplified if !has_hans => Need::Simplified,
        CjkPreference::Traditional if !has_hant => Need::Traditional,
        _ => Need::Broad,
    }
}

/// The installed fallback face for `need`, read once per need. `None` when it is turned off or no
/// candidate fits.
pub fn fallback(need: Need) -> Option<Arc<FontData>> {
    static BROAD: OnceLock<Option<Arc<FontData>>> = OnceLock::new();
    static SIMPLIFIED: OnceLock<Option<Arc<FontData>>> = OnceLock::new();
    static TRADITIONAL: OnceLock<Option<Arc<FontData>>> = OnceLock::new();
    let cache = match need {
        Need::Broad => &BROAD,
        Need::Simplified => &SIMPLIFIED,
        Need::Traditional => &TRADITIONAL,
    };
    cache.get_or_init(|| load(need)).clone()
}

fn load(need: Need) -> Option<Arc<FontData>> {
    if std::env::var_os("PDFCRAFT_SYSTEM_FONTS").is_some_and(|v| v == "0") {
        return None;
    }
    candidates(need).iter().find_map(|path| read(path, need))
}

/// Well-known locations of installed faces for `need`, best first. Chinese candidates are the
/// operating systems' own UI faces and the common free ones; Source Han and Noto CJK (an Adobe
/// design, AGENTS.md §1.1) are deliberately not among them.
fn candidates(need: Need) -> Vec<PathBuf> {
    if cfg!(windows) {
        let dir = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")).map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let files: &[&str] = match need {
            Need::Broad => &["segoeui.ttf", "tahoma.ttf", "arial.ttf"],
            Need::Simplified => &["msyh.ttc", "Deng.ttf", "simsun.ttc", "msjh.ttc"],
            Need::Traditional => &["msjh.ttc", "mingliu.ttc", "msyh.ttc"],
        };
        files.iter().map(|f| dir.join("Fonts").join(f)).collect()
    } else if cfg!(target_os = "macos") {
        let files: &[&str] = match need {
            Need::Broad => {
                &["/System/Library/Fonts/SFArabic.ttf", "/System/Library/Fonts/GeezaPro.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf"]
            }
            Need::Simplified => {
                &["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/STHeiti Light.ttc"]
            }
            Need::Traditional => {
                &["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/STHeiti Light.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"]
            }
        };
        files.iter().map(PathBuf::from).collect()
    } else {
        let files: &[&str] = match need {
            Need::Broad => &[
                "truetype/noto/NotoSansArabic-Regular.ttf",
                "noto/NotoSansArabic-Regular.ttf",
                "google-noto/NotoSansArabic-Regular.ttf",
                "truetype/dejavu/DejaVuSans.ttf",
                "TTF/DejaVuSans.ttf",
                "dejavu/DejaVuSans.ttf",
                "dejavu-sans-fonts/DejaVuSans.ttf",
            ],
            // WenQuanYi and Droid Sans Fallback cover both Chinese scripts.
            Need::Simplified | Need::Traditional => &[
                "truetype/wqy/wqy-microhei.ttc",
                "wqy-microhei/wqy-microhei.ttc",
                "wenquanyi/wqy-microhei/wqy-microhei.ttc",
                "truetype/droid/DroidSansFallbackFull.ttf",
                "google-droid-sans-fonts/DroidSansFallbackFull.ttf",
                "droid/DroidSansFallbackFull.ttf",
                "TTF/DroidSansFallbackFull.ttf",
                "truetype/wqy/wqy-zenhei.ttc",
                "wqy-zenhei/wqy-zenhei.ttc",
                "wenquanyi/wqy-zenhei/wqy-zenhei.ttc",
            ],
        };
        ["/usr/share/fonts", "/usr/local/share/fonts"].iter().flat_map(|dir| files.iter().map(move |f| Path::new(dir).join(f))).collect()
    }
}

fn read(path: &Path, need: Need) -> Option<Arc<FontData>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > need.max_bytes() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let index = face_with(&bytes, need.probe())?;
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    log::info!("interface font fallback ({need:?}): {}", path.display());
    Some(Arc::new(data))
}

/// The first face of the file that parses and maps `c`. egui parses fonts with the same skrifa,
/// so a face accepted here is one it can load.
fn face_with(bytes: &[u8], c: char) -> Option<u32> {
    use skrifa::MetadataProvider as _;
    (0..MAX_FACES).find(|&index| skrifa::FontRef::from_index(bytes, index).is_ok_and(|font| font.charmap().map(c).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEEDS: [Need; 3] = [Need::Broad, Need::Simplified, Need::Traditional];

    #[test]
    fn broken_and_missing_files_are_skipped() {
        for need in NEEDS {
            assert_eq!(face_with(b"", need.probe()), None);
            assert_eq!(face_with(b"not a font at all", need.probe()), None);
            assert_eq!(face_with(&[0u8; 4096], need.probe()), None);
            assert!(read(Path::new("definitely/not/here.ttf"), need).is_none());
            // A directory is not a font.
            assert!(read(&std::env::temp_dir(), need).is_none());
        }
    }

    #[test]
    fn a_face_without_the_probe_is_rejected() {
        // Inter is Latin, Greek and Cyrillic only.
        let inter = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
        for need in NEEDS {
            assert_eq!(face_with(inter, need.probe()), None, "{need:?}");
        }
        assert_eq!(face_with(inter, 'A'), Some(0));
        // The embedded Chinese face, when the build has one, passes both Chinese probes.
        if let Some(zh) = pdfcraft_fonts::ui_simplified_chinese_fonts().first() {
            assert_eq!(face_with(zh.bytes, Need::Simplified.probe()), Some(0));
            assert_eq!(face_with(zh.bytes, Need::Traditional.probe()), Some(0));
        }
    }

    #[test]
    fn candidates_are_absolute_font_files_and_never_an_adobe_design() {
        for need in NEEDS {
            let list = candidates(need);
            assert!(!list.is_empty(), "{need:?}");
            assert!(list.iter().all(|p| p.is_absolute() || cfg!(windows)), "{need:?}: {list:?}");
            assert!(list.iter().all(|p| p.extension().is_some_and(|e| e == "ttf" || e == "ttc")), "{need:?}: {list:?}");
            assert!(
                list.iter().all(|p| !p.to_string_lossy().to_ascii_lowercase().contains("sourcehan") && !p.to_string_lossy().contains("CJK")),
                "{need:?}: {list:?}"
            );
        }
    }

    /// A Chinese face is looked for only in a Chinese interface the build has no face for; every
    /// other case keeps the broad fallback (#882).
    #[test]
    fn the_face_follows_what_the_interface_lacks() {
        use CjkPreference::{Japanese, Simplified, Traditional};
        assert_eq!(need_for(Simplified, false, false), Need::Simplified);
        assert_eq!(need_for(Traditional, false, false), Need::Traditional);
        assert_eq!(need_for(Simplified, true, true), Need::Broad, "a release embeds the face");
        assert_eq!(need_for(Traditional, true, false), Need::Traditional);
        assert_eq!(need_for(Simplified, false, true), Need::Simplified);
        assert_eq!(need_for(Japanese, false, false), Need::Broad);
    }
}
