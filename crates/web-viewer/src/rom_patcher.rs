//! In-browser randomizer / disc patcher.
//!
//! Runs the Track-1 [`legaia_patcher`] randomizer entirely client-side: the user
//! supplies their own disc image, the patcher edits it in WASM memory, and the
//! page downloads the patched image locally. No bytes leave the browser and
//! nothing is uploaded - the same "user supplies the disc" model as the CLI, so
//! the site still ships only code.
//!
//! [`patch_rom`] returns a JS object `{ data: Uint8Array, summary: String,
//! seed: String }`: `data` is the patched image (the download), `summary` is a
//! human-readable change report, `seed` is the resolved numeric seed (so a run
//! reproduces from a memorable string seed).

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;
use legaia_patcher::items::valid_item_pool;
use legaia_patcher::rng::seed_from_str;
use legaia_patcher::translation::{
    ImportPhase, ImportReport, LanguagePack, export_pack, import_pack, import_pack_phase,
    import_pack_relayout, lift,
};

mod lang;
mod patch;
mod tables;
mod textures;

pub use lang::*;
pub use patch::*;
pub use tables::*;
pub use textures::*;

fn parse_mode(s: &str) -> Option<DropMode> {
    match s {
        "shuffle" => Some(DropMode::Shuffle),
        "random" => Some(DropMode::Random),
        _ => None, // "none" or anything else
    }
}

fn parse_encounter_scope(s: &str) -> apply::EncounterScope {
    match s {
        "kingdom" => apply::EncounterScope::Kingdom,
        "world" => apply::EncounterScope::World,
        _ => apply::EncounterScope::Scene, // "scene" or anything else
    }
}

fn err(msg: impl AsRef<str>) -> JsValue {
    JsValue::from_str(msg.as_ref())
}

/// Resolve after one `setTimeout(..., 0)` macrotask, so the browser can
/// repaint between the synchronous patch stages. A microtask
/// (`Promise.resolve()`) is not enough - the renderer only paints at a
/// macrotask boundary. Looked up via the global object so it works in both
/// window and worker scopes; a scope without `setTimeout` resolves
/// immediately (no paint, but the patch still completes).
async fn macrotask_yield() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let global = js_sys::global();
        let scheduled = Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call2(&global, &resolve, &JsValue::from(0)).ok());
        if scheduled.is_none() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Stage-progress reporter for the async patch entry points. Holds the
/// page-supplied JS callback (when one is passed); each [`Progress::stage`]
/// invokes it with `(stage_index, stage_count, label)` and then yields one
/// macrotask so the page's bar actually paints. Stages are the feature
/// blocks the patch applies sequentially - a couple dozen yields per run,
/// never one per inner-loop item. Without a callback nothing is reported
/// and nothing yields.
struct Progress {
    callback: Option<js_sys::Function>,
    index: u32,
    count: u32,
}

impl Progress {
    fn new(callback: Option<js_sys::Function>, count: u32) -> Self {
        Self {
            callback,
            index: 0,
            count,
        }
    }

    async fn stage(&mut self, label: &str) {
        let idx = self.index;
        self.index += 1;
        let Some(cb) = &self.callback else { return };
        let _ = cb.call3(
            &JsValue::NULL,
            &JsValue::from(idx),
            &JsValue::from(self.count),
            &JsValue::from_str(label),
        );
        macrotask_yield().await;
    }
}

/// Parse one arts-AP token: `[CHARACTER:]COMBO=AMOUNT` (`Vahn:RDLDL=10`,
/// `RDLDL=10`). `grant` picks the mode. Returns `None` on any malformed token
/// (the caller reports it in the summary and carries on).
fn parse_art_ap_token(tok: &str, grant: bool) -> Option<legaia_patcher::arts_ap_grant::ArtApSpec> {
    use legaia_art::queue::Character;
    use legaia_patcher::arts_ap_grant::{AP_CAP, ApMode, ArtApSpec};
    let (lhs, val_str) = tok.trim().split_once('=')?;
    let (character, combo_str) = match lhs.split_once(':') {
        Some((c, rest)) => {
            let ch = match c.trim().to_ascii_lowercase().as_str() {
                "vahn" => Character::Vahn,
                "noa" => Character::Noa,
                "gala" => Character::Gala,
                _ => return None,
            };
            (Some(ch), rest)
        }
        None => (None, lhs),
    };
    let combo = legaia_patcher::arts_power::parse_combo(combo_str.trim())?;
    let vs = val_str.trim();
    let amount = vs
        .strip_prefix("0x")
        .or_else(|| vs.strip_prefix("0X"))
        .map(|h| u8::from_str_radix(h, 16))
        .unwrap_or_else(|| vs.parse::<u8>())
        .ok()?;
    if amount < 1 || u16::from(amount) > AP_CAP {
        return None;
    }
    Some(ArtApSpec {
        character,
        combo,
        mode: if grant {
            ApMode::Grant(amount)
        } else {
            ApMode::Cost(amount)
        },
    })
}

/// Parse an `item=value` pair where `item` is a u8 id (decimal or `0xHH`) and
/// `value` is a u32. Returns `None` on any malformed token.
fn parse_id_eq_u32(tok: &str) -> Option<(u8, u32)> {
    let (id_str, val_str) = tok.trim().split_once('=')?;
    let id_str = id_str.trim();
    let id = if let Some(hex) = id_str
        .strip_prefix("0x")
        .or_else(|| id_str.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16).ok()?
    } else {
        id_str.parse::<u8>().ok()?
    };
    let value = val_str.trim().parse::<u32>().ok()?;
    Some((id, value))
}
