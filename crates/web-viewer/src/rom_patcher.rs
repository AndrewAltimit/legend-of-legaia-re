//! In-browser randomizer / disc patcher.
//!
//! Runs the Track-1 [`legaia_rando`] randomizer entirely client-side: the user
//! supplies their own disc image, the patcher edits it in WASM memory, and the
//! page downloads the patched image locally. No bytes leave the browser and
//! nothing is uploaded — the same "user supplies the disc" model as the CLI, so
//! the site still ships only code.
//!
//! [`patch_rom`] returns a JS object `{ data: Uint8Array, summary: String,
//! seed: String }`: `data` is the patched image (the download), `summary` is a
//! human-readable change report, `seed` is the resolved numeric seed (so a run
//! reproduces from a memorable string seed).

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::rng::seed_from_str;

fn parse_mode(s: &str) -> Option<DropMode> {
    match s {
        "shuffle" => Some(DropMode::Shuffle),
        "random" => Some(DropMode::Random),
        _ => None, // "none" or anything else
    }
}

fn err(msg: impl AsRef<str>) -> JsValue {
    JsValue::from_str(msg.as_ref())
}

/// Resolve a user seed string to the numeric seed, as a decimal string (so the
/// page can display / persist it without JS `BigInt` precision loss).
#[wasm_bindgen]
pub fn resolve_seed(seed: &str) -> String {
    seed_from_str(seed).to_string()
}

/// Patch a user-supplied disc image with the chosen randomizer settings.
///
/// `drops` / `encounters` / `chests` are each `"shuffle"`, `"random"`, or
/// `"none"`. `seed` is a number or any string (hashed). Returns
/// `{ data, summary, seed }`.
#[wasm_bindgen]
pub fn patch_rom(
    image: Vec<u8>,
    seed: &str,
    drops: &str,
    encounters: &str,
    chests: &str,
) -> Result<JsValue, JsValue> {
    let seed_n = seed_from_str(seed);
    let drops_mode = parse_mode(drops);
    let enc_mode = parse_mode(encounters);
    let chest_mode = parse_mode(chests);

    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;

    // The valid item pool (from SCUS) is needed only by the `random` modes.
    let pool = if drops_mode == Some(DropMode::Random) || chest_mode == Some(DropMode::Random) {
        let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .ok_or_else(|| err("SCUS_942.54 not found in disc image (needed for a random mode)"))?;
        valid_item_pool(&scus).map_err(|e| err(format!("item pool: {e}")))?
    } else {
        Vec::new()
    };

    let mut summary = String::new();

    match drops_mode {
        Some(m) => {
            let (plan, rep) = apply::randomize_drops(&mut patcher, &pool, seed_n, m)
                .map_err(|e| err(format!("drops: {e}")))?;
            summary.push_str(&format!(
                "drops: {} of {} reassigned ({})\n",
                rep.changed,
                plan.len(),
                drops
            ));
            if !rep.skipped.is_empty() {
                summary.push_str(&format!(
                    "  {} slot(s) too full to re-pack\n",
                    rep.skipped.len()
                ));
            }
        }
        None => summary.push_str("drops: untouched\n"),
    }

    match enc_mode {
        Some(m) => {
            let rep = apply::randomize_encounters(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("encounters: {e}")))?;
            summary.push_str(&format!(
                "encounters: {} scenes, {} ids changed ({})\n",
                rep.scenes_changed, rep.ids_changed, encounters
            ));
        }
        None => summary.push_str("encounters: untouched\n"),
    }

    match chest_mode {
        Some(m) => {
            let rep = apply::randomize_chests(&mut patcher, &pool, seed_n, m)
                .map_err(|e| err(format!("chests: {e}")))?;
            summary.push_str(&format!(
                "chests: {} of {} sites changed across {} scenes ({})\n",
                rep.items_changed, rep.sites_total, rep.scenes_changed, chests
            ));
        }
        None => summary.push_str("chests: untouched\n"),
    }

    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);

    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Reflect::set(&out, &"seed".into(), &seed_n.to_string().into())?;
    Ok(out.into())
}
