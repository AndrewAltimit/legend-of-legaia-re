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

/// Parse a `--fishing-price` entry: `ITEM=POINTS` (`0x6F=500`, `111=1000`). The
/// item id is the SCUS item-name id space; the price is fishing points
/// (`u32`). Errors on a malformed pair.
pub(crate) fn parse_prize_price(s: &str) -> Result<(u8, u32)> {
    let s = s.trim();
    let (id_str, price_str) = s.split_once('=').with_context(|| {
        format!("invalid fishing price {s:?} (expected ITEM=POINTS, e.g. 0x6F=500)")
    })?;
    let price = price_str
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid points in {s:?} (expected a number)"))?;
    Ok((parse_item_id(id_str)?, price))
}

/// Parse an `--enemy-stat-scale` value, in any of its spellings:
///
/// - one multiplier for every stat - `2.5` (a trailing `x` is accepted, so
///   `2.5x` works too);
/// - a per-stat list - `hp=2,attack=1.5` - where unnamed stats stay at retail;
/// - a per-group split - `regular:1|boss:2.5` - where each half of the roster
///   (random encounters vs scripted boss fights) takes its own scale, and each
///   half's body is either of the two spellings above.
///
/// Each multiplier is rounded to the nearest thousandth and range-checked to
/// `0.1x..=5x`. Delegates to
/// [`legaia_patcher::monster_stats::ScaleProfile::parse`], the same parser the
/// browser's simple and advanced slider panes use, so every front-end accepts
/// exactly the same values and emits the same bytes.
pub(crate) fn parse_stat_scale(s: &str) -> Result<legaia_patcher::monster_stats::ScaleProfile> {
    legaia_patcher::monster_stats::ScaleProfile::parse(s).map_err(|e| anyhow::anyhow!(e))
}

/// Parse an `--exp-scale` multiplier (`"2"`, `"0.5x"`; `0.1..=5`). Shares
/// [`ScalePermille::parse`](legaia_patcher::monster_stats::ScalePermille::parse)
/// with the difficulty scale and the browser slider, so every front-end
/// accepts exactly the same values and emits the same bytes.
pub(crate) fn parse_exp_scale(s: &str) -> Result<legaia_patcher::monster_stats::ScalePermille> {
    legaia_patcher::monster_stats::ScalePermille::parse(s).map_err(|e| anyhow::anyhow!(e))
}

/// Parse a `--seru-catch-rate` percent (`"55"`, `"100%"`; `0..=100`). Shared
/// with the browser slider via [`legaia_patcher::rewards::parse_catch_rate`].
pub(crate) fn parse_seru_catch_rate(s: &str) -> Result<u8> {
    legaia_patcher::rewards::parse_catch_rate(s).map_err(|e| anyhow::anyhow!(e))
}

/// Parse an `--enemy-attack-count` multiplier (`"2"`, `"0.5x"`; `0.1..=5`).
/// Shares
/// [`ScalePermille::parse`](legaia_patcher::monster_stats::ScalePermille::parse)
/// with the difficulty scale and the browser slider, so every front-end
/// accepts exactly the same values and emits the same bytes.
pub(crate) fn parse_attack_count_scale(
    s: &str,
) -> Result<legaia_patcher::monster_stats::ScalePermille> {
    legaia_patcher::monster_stats::ScalePermille::parse(s).map_err(|e| anyhow::anyhow!(e))
}

/// Parse an `--arts-power` entry: `COMBO=VALUE` (`RDLDL=0x16`). The combo is a
/// run of `L/R/D/U` glyphs (case-insensitive); `VALUE` is a power-encoding byte
/// (`0` to disable, or `0x0C..=0x1F` for a real damage tier), given in decimal
/// or `0xHH`. Errors on a malformed pair, an unknown glyph, or an
/// out-of-range value.
pub(crate) fn parse_arts_power(s: &str) -> Result<(Vec<legaia_art::queue::Command>, u8)> {
    let s = s.trim();
    let (combo_str, val_str) = s.split_once('=').with_context(|| {
        format!("invalid arts power {s:?} (expected COMBO=VALUE, e.g. RDLDL=0x16)")
    })?;
    let combo = legaia_patcher::arts_power::parse_combo(combo_str.trim())
        .with_context(|| format!("invalid combo {combo_str:?} (use L/R/D/U glyphs, e.g. RDLDL)"))?;
    let vs = val_str.trim();
    let value = if let Some(hex) = vs.strip_prefix("0x").or_else(|| vs.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        vs.parse::<u8>()
    }
    .with_context(|| format!("invalid power value in {s:?} (expected 0 or 0x0C..=0x1F)"))?;
    if value != 0 && !legaia_patcher::arts_power::is_power_byte(value) {
        anyhow::bail!(
            "power value {value:#04x} is not a damage tier: use 0 (disable) or 0x0C..=0x1F"
        );
    }
    Ok((combo, value))
}

/// Parse a `--super-art-power` entry: `[CHARACTER:]NAME=VALUE`
/// (`Tri-Somersault=0x1A`, `Vahn:tri somersault=0x1A`). The name match ignores
/// case, spaces and punctuation. `VALUE` is a power-encoding byte
/// (`0x0C..=0x1F`) or `0` to disable the Super Art's hits.
pub(crate) fn parse_super_art_power(s: &str) -> Result<(&'static legaia_art::SuperArt, u8)> {
    use legaia_art::queue::Character;
    let s = s.trim();
    let (name_str, val_str) = s.split_once('=').with_context(|| {
        format!(
            "invalid super-art power {s:?} \
             (expected [CHARACTER:]NAME=VALUE, e.g. \"Tri-Somersault\"=0x1A)"
        )
    })?;
    let (character, name_str) = match name_str.split_once(':') {
        Some((c, rest)) => {
            let ch = match c.trim().to_ascii_lowercase().as_str() {
                "vahn" => Character::Vahn,
                "noa" => Character::Noa,
                "gala" => Character::Gala,
                other => anyhow::bail!("unknown character {other:?} (use Vahn, Noa or Gala)"),
            };
            (Some(ch), rest)
        }
        None => (None, name_str),
    };
    let matches = legaia_patcher::super_art_power::find_super_art(name_str.trim(), character);
    let art = match matches.len() {
        1 => matches[0],
        0 => anyhow::bail!(
            "no Super Art named {:?} (the fifteen are: {})",
            name_str.trim(),
            legaia_art::SUPER_ARTS
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        n => anyhow::bail!(
            "{:?} matches {n} Super Arts - add a CHARACTER: prefix",
            name_str.trim()
        ),
    };
    let vs = val_str.trim();
    let value = if let Some(hex) = vs.strip_prefix("0x").or_else(|| vs.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        vs.parse::<u8>()
    }
    .with_context(|| format!("invalid power value in {s:?} (expected 0 or 0x0C..=0x1F)"))?;
    if !legaia_patcher::super_art_power::is_accepted_power(value) {
        anyhow::bail!(
            "power value {value:#04x} is not a damage tier: use 0 (disable) or 0x0C..=0x1F"
        );
    }
    Ok((art, value))
}

/// Parse an arts-AP entry: `[CHARACTER:]COMBO=AMOUNT` (`RDLDL=10`,
/// `Vahn:RDLDL=10`). The optional character prefix (`Vahn`/`Noa`/`Gala`,
/// case-insensitive) narrows the override to that character's art; without it
/// every character holding the combo is targeted (each gets its own config
/// cell - nothing is shared). The combo is a run of `L/R/D/U` glyphs; `AMOUNT`
/// is `1..=100`, decimal or `0xHH`.
fn parse_arts_ap_entry(
    s: &str,
    what: &str,
) -> Result<(
    Option<legaia_art::queue::Character>,
    Vec<legaia_art::queue::Command>,
    u8,
)> {
    use legaia_art::queue::Character;
    let s = s.trim();
    let (combo_str, val_str) = s.split_once('=').with_context(|| {
        format!("invalid arts {what} {s:?} (expected [CHARACTER:]COMBO=AMOUNT, e.g. RDLDL=10)")
    })?;
    let (character, combo_str) = match combo_str.split_once(':') {
        Some((c, rest)) => {
            let ch = match c.trim().to_ascii_lowercase().as_str() {
                "vahn" => Character::Vahn,
                "noa" => Character::Noa,
                "gala" => Character::Gala,
                other => anyhow::bail!("unknown character {other:?} (use Vahn, Noa or Gala)"),
            };
            (Some(ch), rest)
        }
        None => (None, combo_str),
    };
    let combo = legaia_patcher::arts_power::parse_combo(combo_str.trim())
        .with_context(|| format!("invalid combo {combo_str:?} (use L/R/D/U glyphs, e.g. RDLDL)"))?;
    let vs = val_str.trim();
    let amount = if let Some(hex) = vs.strip_prefix("0x").or_else(|| vs.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        vs.parse::<u8>()
    }
    .with_context(|| format!("invalid AP amount in {s:?} (expected 1..=100)"))?;
    if amount < 1 || u16::from(amount) > legaia_patcher::arts_ap_grant::AP_CAP {
        anyhow::bail!("arts {what} amount {amount} out of range (use 1..=100)");
    }
    Ok((character, combo, amount))
}

/// Parse an `--arts-ap-grant` entry into a [`legaia_patcher::arts_ap_grant::ArtApSpec`].
pub(crate) fn parse_arts_ap_grant(s: &str) -> Result<legaia_patcher::arts_ap_grant::ArtApSpec> {
    let (character, combo, amount) = parse_arts_ap_entry(s, "AP-grant")?;
    Ok(legaia_patcher::arts_ap_grant::ArtApSpec {
        character,
        combo,
        mode: legaia_patcher::arts_ap_grant::ApMode::Grant(amount),
    })
}

/// Parse an `--arts-ap-cost` entry into a [`legaia_patcher::arts_ap_grant::ArtApSpec`].
pub(crate) fn parse_arts_ap_cost(s: &str) -> Result<legaia_patcher::arts_ap_grant::ArtApSpec> {
    let (character, combo, amount) = parse_arts_ap_entry(s, "AP-cost")?;
    Ok(legaia_patcher::arts_ap_grant::ArtApSpec {
        character,
        combo,
        mode: legaia_patcher::arts_ap_grant::ApMode::Cost(amount),
    })
}

/// Parse a `--rename-location` entry: `TARGET=NAME`, where `TARGET` is either a
/// landmark cell index (`3=Ancient Fire Cave`) or the place's current name
/// (`Hunter's Spring=Hunter's Well`). A purely numeric target is always read as
/// an index. Errors on a malformed pair; name validity is checked at apply time.
pub(crate) fn parse_location_rename(
    s: &str,
) -> Result<(legaia_patcher::apply::RenameTarget, String)> {
    let (target, name) = s
        .split_once('=')
        .with_context(|| format!("invalid location rename {s:?} (expected TARGET=NAME)"))?;
    if target.trim().is_empty() {
        anyhow::bail!("invalid location rename {s:?} (empty target before `=`)");
    }
    Ok((
        legaia_patcher::apply::RenameTarget::parse(target),
        name.to_string(),
    ))
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

/// Parse a `--delilas-party` mapping: three comma-separated sibling names in
/// Vahn, Noa, Gala order, each used once (`gi,lu,che`).
pub(crate) fn parse_delilas_party(s: &str) -> Result<legaia_patcher::delilas_party::PartyMapping> {
    legaia_patcher::delilas_party::PartyMapping::parse(s)
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
