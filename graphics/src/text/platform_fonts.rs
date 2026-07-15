//! Targeted, cheap loading of the platform's default UI font(s), avoiding a
//! full parse of every installed system font.
//!
//! `fontdb::Database::load_system_fonts()` recursively reads and *parses*
//! every font file it finds under the OS's font directories. On a machine
//! with a large font collection this can take several seconds, and on a
//! cold disk cache (e.g. right after a reboot) it can take much longer —
//! see <https://github.com/iced-rs/iced/issues/2455>.
//!
//! In practice this application only ever needs one platform default UI font
//! plus its own bundled fonts. [`load_platform_ui_fonts`] performs a cheap
//! directory listing (no parsing) looking only for filenames that match what
//! we actually need, and parses just those. If none are found, the caller
//! should fall back to [`cosmic_text::fontdb::Database::load_system_fonts`]
//! so nothing is ever silently missing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cosmic_text::fontdb;

/// Recursively finds files under `dirs` whose filename (stem, lowercased,
/// alphanumeric characters only) starts with one of `name_prefixes`, and
/// loads each match into `db`. Returns the number of files successfully
/// loaded. Skips symlinks to avoid loops.
fn load_matching_fonts(db: &mut fontdb::Database, dirs: &[PathBuf], name_prefixes: &[&str]) -> usize {
    let mut seen = HashSet::new();
    let mut loaded = 0;

    for dir in dirs {
        loaded += load_matching_fonts_dir(db, dir, name_prefixes, &mut seen);
    }

    loaded
}

fn load_matching_fonts_dir(
    db: &mut fontdb::Database,
    dir: &Path,
    name_prefixes: &[&str],
    seen: &mut HashSet<PathBuf>,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };

    let mut loaded = 0;

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        // Skip symlinks entirely: this is just an optimization pass, and
        // the fallback (`load_system_fonts`) already covers whatever a
        // symlink might have pointed at.
        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();

        if file_type.is_dir() {
            loaded += load_matching_fonts_dir(db, &path, name_prefixes, seen);
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        match path.extension().and_then(|e| e.to_str()) {
            Some("ttf") | Some("ttc") | Some("otf") | Some("otc") | Some("TTF") | Some("TTC")
            | Some("OTF") | Some("OTC") => {}
            _ => continue,
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        let normalized: String = stem
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect();

        if !name_prefixes.iter().any(|prefix| normalized.starts_with(prefix)) {
            continue;
        }

        if !seen.insert(path.clone()) {
            continue;
        }

        match db.load_font_file(&path) {
            Ok(()) => loaded += 1,
            Err(e) => {
                log::warn!("Failed to load font '{}': {e}", path.display());
            }
        }
    }

    loaded
}

/// Loads the platform's default UI font(s) directly, without scanning every
/// installed system font. Returns `true` if the app's default font is now
/// available in `db`, `false` if the caller should fall back to a full
/// [`fontdb::Database::load_system_fonts`] scan.
#[cfg(target_os = "windows")]
pub fn load_platform_ui_fonts(db: &mut fontdb::Database) -> bool {
    let fonts_dir = std::env::var_os("SYSTEMROOT")
        .map(|root| PathBuf::from(root).join("Fonts"))
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Fonts"));

    // Matches segoeui.ttf (Regular), segoeuib.ttf (Bold), segoeuii.ttf
    // (Italic), segoeuiz.ttf (Bold Italic) — the app's default font,
    // read directly from the OS's own install (never bundled/redistributed
    // by us).
    load_matching_fonts(db, &[fonts_dir], &["segoeui"]) > 0
}

/// Loads the platform's default UI font(s) directly, without scanning every
/// installed system font. Returns `true` if the app's default font is now
/// available in `db`, `false` if the caller should fall back to a full
/// [`fontdb::Database::load_system_fonts`] scan.
#[cfg(target_os = "macos")]
pub fn load_platform_ui_fonts(db: &mut fontdb::Database) -> bool {
    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
    ];

    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join("Library/Fonts"));
    }

    load_matching_fonts(db, &dirs, &["sfpro", "sfns"]);

    // "SF Pro" is requested as an exact family name, so a near-miss file
    // isn't good enough -- confirm the family is actually present.
    db.faces()
        .any(|face| face.families.iter().any(|(name, _)| name == "SF Pro"))
}

/// Loads the platform's default UI font(s) directly, without scanning every
/// installed system font. Returns `true` if a usable fallback font is now
/// available in `db`, `false` if the caller should fall back to a full
/// [`fontdb::Database::load_system_fonts`] scan.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn load_platform_ui_fonts(db: &mut fontdb::Database) -> bool {
    let mut dirs = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];

    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".fonts"));
        dirs.push(PathBuf::from(&home).join(".local/share/fonts"));
    }

    // The app's own UI text already renders via the bundled Fira Sans font
    // (see `font_system()`), but cosmic-text's fallback mechanism for
    // glyphs outside Fira Sans's coverage (e.g. CJK/Cyrillic/Arabic device
    // names, emoji) only ever returns candidate family name strings -- it
    // never scans the disk itself. So we still need at least one of its
    // expected fallback families (matches cosmic-text's own
    // `common_fallback()` list) actually loaded, or that text would
    // silently fail to render.
    load_matching_fonts(db, &dirs, &["dejavusans", "notosans", "freesans"]) > 0
}
