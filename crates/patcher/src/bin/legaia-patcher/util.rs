//! Small shared helpers: seed / item-id parsing, disc-image loading, the output
//! path + CUE-sheet builders.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Resolve a user seed string to a numeric seed (shared with the in-browser
/// patcher via [`legaia_patcher::rng::seed_from_str`]).
pub(crate) fn resolve_seed(seed: &str) -> u64 {
    legaia_patcher::rng::seed_from_str(seed)
}

/// Parse an item id from a decimal or `0x`-hex string (e.g. `154` or `0x9a`).
pub(crate) fn parse_item_id(s: &str) -> Result<u8> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        s.parse::<u8>()
    };
    parsed.with_context(|| format!("invalid item id {s:?} (expected 0..=255, decimal or 0xHH)"))
}

/// Parse a single `--start-with` entry: an item id, optionally `:count`
/// (`0x89:10`, `0xd1`, `154:3`). Count defaults to `1` and is clamped to the
/// game's per-slot stack cap. The id space is the full 256-id item table
/// (consumables, equipment, AND accessories), so any item can be requested.
pub(crate) fn parse_item_spec(s: &str) -> Result<(u8, u8)> {
    let s = s.trim();
    let (id_str, count) = match s.split_once(':') {
        Some((id_str, count_str)) => {
            let count = count_str
                .trim()
                .parse::<u32>()
                .with_context(|| format!("invalid count in {s:?} (expected a number)"))?;
            (
                id_str,
                count.min(legaia_patcher::starting_items::MAX_ITEM_STACK as u32) as u8,
            )
        }
        None => (s, 1u8),
    };
    Ok((parse_item_id(id_str)?, count))
}

pub(crate) fn clock_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
}

pub(crate) fn load_image(path: &Path) -> Result<Vec<u8>> {
    // A `.cue` sheet is a text index, not the disc data - resolve it to the
    // `.bin` it references so users can pass either.
    let resolved = legaia_iso::raw::resolve_disc_path(path)
        .with_context(|| format!("resolve disc image {}", path.display()))?;
    if resolved != path {
        println!(
            "note: {} is a cue sheet; reading {}",
            path.display(),
            resolved.display()
        );
    }
    std::fs::read(&resolved).with_context(|| format!("read disc image {}", resolved.display()))
}

/// The primary-executable name of the USA disc every randomizer offset / code
/// hook targets.
pub(crate) const USA_EXE: &str = "SCUS_942.54";

/// Human label for a known Legaia primary-executable name.
pub(crate) fn describe_exe(exe: &str) -> String {
    match exe {
        USA_EXE => format!("{exe} (USA)"),
        "SCES_019.44" => format!("{exe} (France, PAL)"),
        "SCES_019.45" => format!("{exe} (Germany, PAL)"),
        "SCES_019.46" => format!("{exe} (Italy, PAL)"),
        other => format!("{other} (unrecognized build)"),
    }
}

/// Detect the disc's primary executable via its `SYSTEM.CNF` `BOOT=` line
/// (ISO9660 walk; works on any Mode 2/2352 PSX image).
pub(crate) fn detect_exe(image: &[u8]) -> Option<String> {
    let cnf = legaia_iso::iso9660::read_file_in_image(image, "SYSTEM.CNF")?;
    legaia_iso::region::parse(&cnf).ok().map(|d| d.executable)
}

/// Region guard: `action` is patched with USA-disc offsets, so hard-error on
/// any non-USA disc unless the user explicitly opted in. Returns the human
/// label of the detected build for callers that want to print it.
pub(crate) fn check_usa_disc(image: &[u8], allow_mismatch: bool, action: &str) -> Result<String> {
    let label = match detect_exe(image) {
        Some(exe) => describe_exe(&exe),
        None => "unknown (SYSTEM.CNF not readable)".to_string(),
    };
    if label.starts_with(USA_EXE) {
        return Ok(label);
    }
    if allow_mismatch {
        println!(
            "warning: {action} targets the USA build ({USA_EXE} / SCUS-94254) but this \
             disc is {label}; proceeding because --allow-region-mismatch was passed"
        );
        return Ok(label);
    }
    anyhow::bail!(
        "{action} targets the USA build ({USA_EXE} / SCUS-94254); found {label}.\n\
         Patching this disc with USA offsets would \"succeed\" but produce a corrupt \
         hybrid image.\nUse a USA disc dump, or pass --allow-region-mismatch if you \
         really know the patch matches this disc."
    )
}

/// One-line notice before clobbering an existing output file (no prompt).
pub(crate) fn note_overwrite(path: &Path) {
    if path.exists() {
        println!("overwriting {}", path.display());
    }
}

/// A single-track Mode 2/2352 CUE sheet pointing at `bin_name` (the patched
/// image's file name). The randomizer only operates on Mode 2/2352 PSX discs, so
/// the one-track layout matches the source disc; `bin_name` is bare (no path) so
/// the CUE stays valid as long as it sits beside the image.
pub(crate) fn cue_contents(bin_name: &str) -> String {
    format!("FILE \"{bin_name}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n")
}

/// `<stem>.<ext>` next to the input path (e.g. `disc.bin` -> `disc.ppf`).
pub(crate) fn with_extension(input: &Path, ext: &str) -> PathBuf {
    let mut p = input.to_path_buf();
    p.set_extension(ext);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_points_at_the_bare_bin_name_as_mode2_2352() {
        let cue = cue_contents("legaia_enemy_ally_100.bin");
        assert_eq!(
            cue,
            "FILE \"legaia_enemy_ally_100.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n"
        );
        // The quoted FILE name has no directory component - the cue must sit
        // beside the image (the MODE2/2352 token's slash is fine).
        let file_line = cue.lines().next().unwrap();
        assert!(!file_line.contains('/'));
    }

    #[test]
    fn output_cue_path_swaps_the_extension() {
        let out = Path::new("/tmp/some dir/patched.bin");
        assert_eq!(
            out.with_extension("cue"),
            Path::new("/tmp/some dir/patched.cue")
        );
        assert_eq!(
            out.file_name().unwrap().to_string_lossy(),
            "patched.bin",
            "cue FILE uses the bare image name, not the full path"
        );
    }

    #[test]
    fn seed_resolution_is_stable_and_parses_numbers() {
        // Numbers are used directly (decimal + hex).
        assert_eq!(resolve_seed("42"), 42);
        assert_eq!(resolve_seed("0x1F"), 0x1F);
        assert_eq!(resolve_seed("0XFF"), 0xFF);
        // A non-numeric string hashes stably (reproducibility contract) and the
        // same string always maps to the same seed.
        let a = resolve_seed("my cool run");
        assert_eq!(a, resolve_seed("my cool run"));
        assert_ne!(a, resolve_seed("my other run"));
        // A string that isn't a bare number doesn't collide with the number path.
        assert_ne!(resolve_seed("42x"), 42);
    }
}
