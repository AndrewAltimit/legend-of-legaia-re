//! Disc-gated oracle for lifting a **fan-patched** disc
//! (`translate lift-official --baseline`), the path a community translation
//! shipped as a binary patch takes.
//!
//! Needs three images: the USA disc (`LEGAIA_DISC_BIN`), the fan-patched disc
//! (`LEGAIA_FANPATCH_DISC_BIN`) and the retail disc that patch was built on
//! (`LEGAIA_FANPATCH_BASE_BIN`) - e.g. the Brazilian Portuguese patch applied
//! to the Spanish disc, and the Spanish disc. Skips + passes when any is unset
//! (no Sony bytes committed / CI).
//!
//! What it pins, each a defect a real fan patch surfaced:
//!
//! - every dialog line pairs, raw carriers included - a chest line the patch
//!   shortened to its item token fails the scan's quality gate on that side
//!   only, and the carrier's ungated framing list is what still pairs it;
//! - every overlay UI pool and SCUS system-string pool pairs - a label the
//!   dialog gate refuses (`p/ equipar`) or an extra label variant mid-pool
//!   used to fail or slide the whole pool;
//! - the baseline filter keeps a line with no prose of its own (a chest line
//!   reduced to its item token): the retail disc carries the same bytes
//!   because the construction is the same in both languages, and blanking it
//!   put the English line back into a translated sentence.

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::lift;

fn load(var: &str) -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os(var)?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A line built only from control tokens and punctuation.
fn token_only(markup: &str) -> bool {
    let mut depth = 0usize;
    !markup.chars().any(|c| {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        depth == 0 && c.is_alphabetic()
    })
}

#[test]
fn fan_patch_lift_pairs_every_carrier_and_keeps_token_lines() {
    let (Some(usa), Some(patched), Some(base)) = (
        load("LEGAIA_DISC_BIN"),
        load("LEGAIA_FANPATCH_DISC_BIN"),
        load("LEGAIA_FANPATCH_BASE_BIN"),
    ) else {
        eprintln!(
            "[skip] LEGAIA_DISC_BIN / LEGAIA_FANPATCH_DISC_BIN / LEGAIA_FANPATCH_BASE_BIN unset"
        );
        return;
    };
    let usa = DiscPatcher::open(usa).expect("open USA disc");
    let patched = DiscPatcher::open(patched).expect("open patched disc");
    let base = DiscPatcher::open(base).expect("open base disc");

    let (mut pack, rep) = lift::lift_official(&usa, &patched).expect("lift the patch");
    assert_eq!(rep.man_paired, rep.man_total, "every MAN line pairs");
    assert_eq!(
        rep.raw_paired, rep.raw_total,
        "every raw-carrier line pairs"
    );
    assert_eq!(rep.ui_paired, rep.ui_total, "every overlay UI string pairs");
    assert_eq!(
        rep.system_paired, rep.system_total,
        "every SCUS system string pairs"
    );

    let (base_pack, _) = lift::lift_official(&usa, &base).expect("lift the base");
    let tokens_before = pack
        .sections
        .iter()
        .flat_map(|(_, es)| es.iter())
        .filter(|e| !e.translation.is_empty() && token_only(&e.translation))
        .count();
    let blanked = lift::drop_baseline_text(&mut pack, &base_pack);
    let tokens_after = pack
        .sections
        .iter()
        .flat_map(|(_, es)| es.iter())
        .filter(|e| !e.translation.is_empty() && token_only(&e.translation))
        .count();
    assert!(blanked > 0, "the patch leaves some retail lines alone");
    assert!(tokens_before > 0, "the patch carries token-only lines");
    assert_eq!(
        tokens_after, tokens_before,
        "the baseline filter blanked a line with no prose of its own"
    );
    eprintln!(
        "[ok] fan patch: {} MAN + {} raw lines, {} UI + {} SCUS strings paired; \
         {blanked} baseline lines blanked, {tokens_after} token-only lines kept",
        rep.man_paired, rep.raw_paired, rep.ui_paired, rep.system_paired
    );
}
