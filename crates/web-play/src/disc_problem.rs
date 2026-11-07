//! Why a picked file is not a disc the page can run, in the desktop programs'
//! words: `psx_iso::sniff` for what the file is, `legaia_app_theme::disc_text`
//! for an image cut short. Native says this before it opens anything; without
//! it the page's only word was the loader's own error.

use legaia_app_theme::disc_text;
use psx_iso::image::walk_iso_source;
use psx_iso::sniff;
use wasm_bindgen::prelude::*;

/// How many leading bytes the page reads for [`disc_file_problem`]: the sniff's
/// own head, and the volume descriptor and directories of a raw image, which
/// sit in the first few dozen sectors. A directory past this reads as no file
/// table, which gives no verdict.
#[wasm_bindgen]
pub fn disc_problem_head_len() -> usize {
    HEAD_SECTORS * 2352
}

const HEAD_SECTORS: usize = 128;

/// The executable of the one disc the engine runs.
const USA_EXECUTABLE: &str = "SCUS_942.54";

/// [`problem`] for the page: two sentences for the player, empty when the file
/// may be tried.
#[wasm_bindgen]
pub fn disc_file_problem(name: &str, head: &[u8], file_len: f64) -> String {
    problem(name, head, file_len as u64).unwrap_or_default()
}

/// What is wrong with the file `name`, from its first bytes and its length.
/// `None` when it looks like a whole raw dump with a `PROT.DAT`, or is a bare
/// `PROT.DAT`, which the page also takes.
pub fn problem(name: &str, head: &[u8], file_len: u64) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower == "prot.dat" {
        return None;
    }
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    if ext == "cue" {
        return Some(
            "This is a cue sheet, which only lists the disc's tracks. \
             Choose the .bin file it names, beside it."
                .into(),
        );
    }
    if let Some(d) = sniff::diagnose_bytes(ext, head, file_len) {
        // The desktop programs follow a .cue; the page reads the .bin alone.
        return Some(
            d.to_string()
                .replace(", or the .cue beside it", "")
                .replace("the .bin or .cue inside", "the .bin inside")
                .replace("the disc's .bin or .cue", "the disc's .bin"),
        );
    }
    let files = walk_iso_source(head);
    // The file table starts past the head: an image too short to hold it.
    if files.is_empty() {
        return (head.len() as u64 >= file_len)
            .then(|| format!("{} {}", disc_text::CUT_SHORT, disc_text::INCOMPLETE_FIX));
    }
    let needed = disc_text::sectors_needed(files.iter().map(|f| (f.lba, f.size)));
    if let Some(what) = disc_text::incomplete_image(file_len, needed) {
        return Some(format!("{what} {}", disc_text::INCOMPLETE_FIX));
    }
    if !files
        .iter()
        .any(|f| f.path.eq_ignore_ascii_case("PROT.DAT"))
    {
        return Some(
            "This disc has no PROT.DAT, so it is not Legend of Legaia. \
             Choose the .bin of your own copy of the game."
                .into(),
        );
    }
    // The desktop programs refuse another region's disc the same way. A patched
    // USA disc keeps its executable and passes.
    if let Some(cnf) = psx_iso::iso9660::read_file_in_source(head, "SYSTEM.CNF")
        && let Ok(detected) = psx_iso::region::parse(&cnf)
        && !detected.executable.eq_ignore_ascii_case(USA_EXECUTABLE)
    {
        return Some(format!(
            "This is the {} release ({}). The engine runs the USA disc ({USA_EXECUTABLE}): \
             choose that one.",
            detected.region.name(),
            detected.executable,
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page asks ahead of its read, and a refused file leaves the cache.
    #[test]
    fn the_page_asks_and_forgets() {
        let page = include_str!("../../../site/_content/play.html");
        assert!(page.contains("wasm.disc_file_problem("));
        assert!(page.contains("disc_problem_head_len()"));
        assert!(page.contains("return { rejected: refused }"));
        let cache = include_str!("../../../site/js/rom-cache.js");
        assert!(cache.contains("v.rejected"));
    }

    #[test]
    fn a_compressed_image_is_named() {
        let p = problem("game.chd", b"MComprHD\0\0\0\0", 5000).unwrap();
        assert!(p.contains("compressed .chd"), "{p}");
    }

    #[test]
    fn a_cue_sheet_points_at_its_bin() {
        let p = problem("game.cue", b"FILE \"game.bin\" BINARY", 60).unwrap();
        assert!(p.contains("the .bin file"), "{p}");
    }

    /// The page's picker takes a .bin alone, so no advice may send a player
    /// back to a .cue: the shared wording is rewritten above, and a change to
    /// it fails here.
    #[test]
    fn no_advice_names_a_cue_the_page_cannot_take() {
        let zeros = [0u8; 4096];
        for (name, head, len) in [
            ("junk.bin", &b"hello"[..], 5),
            ("big.bin", &zeros[..], 4096),
            ("game.zip", &b"PK\x03\x04"[..], 5000),
            ("game.ppf", &b"PPF30"[..], 5000),
        ] {
            let p = problem(name, head, len).unwrap();
            assert!(!p.contains(".cue"), "{name}: {p}");
        }
    }

    #[test]
    fn a_bare_prot_dat_is_tried() {
        assert_eq!(problem("PROT.DAT", b"not a sector", 5000), None);
    }

    /// The disc with its `SYSTEM.CNF` naming the PAL executable. Skips without a
    /// disc.
    #[test]
    fn another_regions_disc_is_refused() {
        let Ok(path) = std::env::var("LEGAIA_DISC_BIN") else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let mut head = bytes[..disc_problem_head_len()].to_vec();
        // Past the directory record of the same name.
        let at = head
            .windows(12)
            .position(|w| w == b"\\SCUS_942.54")
            .expect("the boot line in the head");
        head[at + 1..at + 12].copy_from_slice(b"SCES_019.44");
        let p = problem("disc.bin", &head, bytes.len() as u64).unwrap();
        assert!(p.contains("SCES_019.44"), "{p}");
    }

    /// The disc's own bytes: whole, it is fine; its first sectors alone read
    /// as incomplete, with how much is there. Skips without a disc.
    #[test]
    fn a_cut_image_says_how_much_it_holds() {
        let Ok(path) = std::env::var("LEGAIA_DISC_BIN") else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let head = &bytes[..disc_problem_head_len()];
        assert_eq!(problem("disc.bin", head, bytes.len() as u64), None);
        let cut = (bytes.len() as u64) / 2;
        let p = problem("disc.bin", head, cut).unwrap();
        assert!(p.contains("incomplete"), "{p}");
        assert!(p.contains("only 49%") || p.contains("only 50%"), "{p}");
    }
}
