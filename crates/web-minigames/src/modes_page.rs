//! `LegaiaModes`: the browser game-modes page (`site/modes.html`) - the
//! port's own modes (Hold'em, Grid Duel, golf, racing, prop hunt) played
//! with no disc and no `World`.
//!
//! The modes are `World`-free by construction (`legaia_engine_modes`), and so
//! is the seat that runs one: [`PackModeSeat`] owns the picker, the session
//! in play, its quit prompt and the results card, exactly as it does on the
//! play hosts' `World::expansion`. This class holds one seat, steps it off
//! the page's pad word once a frame (`PackModeSeat::step`, the call
//! `World::tick_pack_mode` makes), and hands the page the seat's screen
//! through the one projection every host draws
//! (`legaia_engine_ui::expansion_modes::seat_screen`): flat panels as quads,
//! labels as unmeasured text the page sets in a font of its own, since the
//! dialog font is disc data.
//!
//! The rules come from the built-in practice set, or from a pack the page
//! loads (Rebirth's tree as the site serves it, or a visitor's `.lxpack`):
//! its `modes.toml`, its `cards.toml` as the duel's card book and its maps,
//! which a golf course or race laid out on a map is built from. With no
//! world to stage such a session in, it draws as the top-down screen.
//! Nothing is granted - a page session has no save to grant into.

use std::collections::BTreeMap;
use std::sync::Arc;

use legaia_engine_expansion::modes::{ModeSource, ModeStage, PackModeSeat, SeatStep};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{
    ActiveMode, BuildOptions, ModeView, builtin_sections, entries, instance_map,
};
use legaia_engine_modes::cards::duel::DuelCard;
use legaia_engine_modes::cards::table_art;
use legaia_engine_modes::session::ModeRng;
use legaia_engine_ui::expansion_modes::{
    Align, mode_screen_labels, mode_screen_prims, seat_screen,
};
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::map::MapDef;
use legaia_expansion_format::modes::ModeSection;
use legaia_expansion_format::{ExpansionPack, PackFiles};
use legaia_render_kernels::screen_prim::{ScreenPrim, order_primitives};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// The rules a seat builds from.
struct Rules {
    source: ModeSource,
    /// The pack's name; empty for the built-in set.
    name: String,
    modes: ModeSection,
    cards: CardSection,
    maps: Arc<BTreeMap<String, MapDef>>,
}

impl Rules {
    fn builtin() -> Self {
        let (modes, cards) = builtin_sections();
        Self {
            source: ModeSource::Builtin,
            name: String::new(),
            modes: modes.clone(),
            cards: cards.clone(),
            maps: Arc::default(),
        }
    }

    fn from_pack(pack: ExpansionPack) -> Result<Self, String> {
        if entries(&pack.modes).is_empty() {
            return Err(format!(
                "{} defines no game modes (modes.toml)",
                pack.manifest.name
            ));
        }
        Ok(Self {
            source: ModeSource::Pack,
            name: pack.manifest.name.clone(),
            modes: pack.modes,
            cards: pack.cards,
            maps: Arc::new(pack.maps),
        })
    }
}

/// The modes page's runtime: one rules source and at most one seat.
#[wasm_bindgen]
pub struct LegaiaModes {
    rules: Rules,
    seat: Option<PackModeSeat>,
    prev_pad: u16,
    /// Bumped every entry, so two sessions of one instance deal differently.
    entries_made: u64,
    /// The page's seed, mixed into every session's.
    seed: u64,
}

impl Default for LegaiaModes {
    fn default() -> Self {
        Self::new()
    }
}

/// A session seed from the page seed, the entry count and the instance id
/// (FNV-1a over the id, as `World::pack_mode_seed` mixes it).
fn session_seed(page: u64, n: u64, id: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in id.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
    }
    h ^ page.rotate_left(29) ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

impl LegaiaModes {
    /// The seat's stage as the page names it.
    fn stage_name(&self) -> &'static str {
        match self.seat.as_ref().map(|s| &s.stage) {
            None => "closed",
            Some(ModeStage::Picker { .. }) => "picker",
            Some(ModeStage::Playing { .. }) => "playing",
            Some(ModeStage::Results { .. }) => "results",
        }
    }

    fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
        self.seat = None;
    }

    /// The open duel's hands, while a duel is being played.
    fn duel_hands(&self) -> Option<[Vec<DuelCard>; 2]> {
        match &self.seat.as_ref()?.stage {
            ModeStage::Playing { mode, .. } => match mode.view() {
                ModeView::Duel(v) => Some(v.hands),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A 15-bit page texel as RGBA8; texel `0` is transparent, as the GPU
/// draws it.
fn texel_rgba(v: u16) -> [u8; 4] {
    if v == 0 {
        return [0; 4];
    }
    let c = |s: u16| {
        let x = ((v >> s) & 31) as u8;
        (x << 3) | (x >> 2)
    };
    [c(0), c(5), c(10), 255]
}

#[wasm_bindgen]
impl LegaiaModes {
    /// A page runtime on the built-in practice set.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            rules: Rules::builtin(),
            seat: None,
            prev_pad: 0,
            entries_made: 0,
            seed: 0x4C45_4741_4941,
        }
    }

    /// Seed every later session (the page passes a clock value, so two
    /// visits deal differently; a test passes a constant).
    pub fn set_seed(&mut self, seed: f64) {
        self.seed = seed as u64;
    }

    /// Back to the built-in practice set (closes any seat).
    pub fn use_builtin(&mut self) {
        self.set_rules(Rules::builtin());
    }

    /// Take the rules from a pack tree: `paths[i]` names the file whose bytes
    /// are `data[i]` (a `Uint8Array`), as the site serves Rebirth
    /// (`packs/<id>.json`). Returns `""`, or why the pack could not be used
    /// (the current rules stay).
    pub fn load_pack_tree(&mut self, paths: Vec<String>, data: js_sys::Array) -> String {
        if paths.len() != data.length() as usize {
            return "paths and data differ in length".into();
        }
        let files: PackFiles = paths
            .into_iter()
            .zip(data.iter())
            .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
            .map(|(p, d)| (p, js_sys::Uint8Array::new(&d).to_vec()))
            .collect();
        self.load_files(&files)
    }

    /// Take the rules from `.lxpack` bundle bytes, read through the format's
    /// import gates (`share::import_bundle`). Returns `""` or the reason.
    pub fn load_pack_bundle(&mut self, bytes: &[u8]) -> String {
        match legaia_expansion_format::share::import_bundle(bytes) {
            Err(e) => e.to_string(),
            Ok(i) => match Rules::from_pack(i.pack) {
                Ok(r) => {
                    self.set_rules(r);
                    String::new()
                }
                Err(e) => e,
            },
        }
    }

    /// `{"source": "builtin"|"pack", "pack": name, "modes": [{"id", "name",
    /// "kind", "map"}]}` - `map` the pack map a golf course, race or table
    /// is laid out on (drawn top-down here), or `null`.
    pub fn modes_json(&self) -> String {
        let modes: Vec<_> = entries(&self.rules.modes)
            .into_iter()
            .map(|e| {
                let map = instance_map(&self.rules.modes, &e.id).map(|(m, _)| m);
                json!({ "id": e.id, "name": e.name, "kind": e.kind.label(), "map": map })
            })
            .collect();
        json!({
            "source": match self.rules.source {
                ModeSource::Pack => "pack",
                ModeSource::Builtin => "builtin",
            },
            "pack": self.rules.name,
            "modes": modes,
        })
        .to_string()
    }

    /// Start instance `id`. Returns `""`, or why it would not build.
    pub fn enter(&mut self, id: &str) -> String {
        self.entries_made += 1;
        let seed = session_seed(self.seed, self.entries_made, id);
        let r = &self.rules;
        match ActiveMode::build_with_maps(
            &r.modes,
            &r.cards,
            &r.maps,
            id,
            seed,
            &BuildOptions::default(),
        ) {
            Ok(mode) => {
                let seat = PackModeSeat::playing(SceneMode::Field, r.source, mode)
                    .with_maps(r.maps.clone());
                self.seat = Some(seat);
                String::new()
            }
            Err(e) => e.to_string(),
        }
    }

    /// Open the in-game picker over the current rules.
    pub fn open_picker(&mut self) {
        let r = &self.rules;
        self.seat = Some(
            PackModeSeat::picker(SceneMode::Field, r.source, &r.modes).with_maps(r.maps.clone()),
        );
    }

    /// Close the seat (a session in play is forfeited).
    pub fn leave(&mut self) {
        self.seat = None;
    }

    /// One frame off the page's pad word (`PadButton` bits). Returns the
    /// stage after it: `"closed"`, `"picker"`, `"playing"` or `"results"`.
    pub fn tick(&mut self, pad: u16) -> String {
        let prev = std::mem::replace(&mut self.prev_pad, pad);
        let Some(seat) = self.seat.as_mut() else {
            return "closed".into();
        };
        let id = match &seat.stage {
            ModeStage::Picker {
                entries, cursor, ..
            } => entries
                .get(*cursor)
                .map(|e| e.id.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let seed = session_seed(self.seed, self.entries_made + 1, &id);
        let mut rng = ModeRng::new(seed ^ 0x5EED);
        let r = &self.rules;
        let step = seat.step(
            pad,
            prev,
            &r.modes,
            &r.cards,
            &BuildOptions::default(),
            seed,
            &mut rng,
        );
        match step {
            SeatStep::Leave => self.seat = None,
            SeatStep::Settled { .. } | SeatStep::Stay => {
                if matches!(
                    self.seat.as_ref().map(|s| &s.stage),
                    Some(ModeStage::Playing { .. })
                ) && !id.is_empty()
                {
                    // The picker just built a session.
                    self.entries_made += 1;
                }
            }
        }
        self.stage_name().into()
    }

    /// The seat's state for the page's own chrome: `{"stage", "id", "name",
    /// "kind", "quit_prompt", "finished"}`.
    pub fn state_json(&self) -> String {
        let (id, name, kind, quit) = match self.seat.as_ref().map(|s| &s.stage) {
            Some(ModeStage::Playing { mode, quit_prompt }) => (
                mode.id.clone(),
                mode.name.clone(),
                mode.kind().label(),
                *quit_prompt,
            ),
            Some(ModeStage::Results { name, kind, .. }) => {
                (String::new(), name.clone(), kind.label(), false)
            }
            _ => (String::new(), String::new(), "", false),
        };
        json!({
            "stage": self.stage_name(),
            "id": id,
            "name": name,
            "kind": kind,
            "quit_prompt": quit,
            "finished": matches!(
                self.seat.as_ref().map(|s| &s.stage),
                Some(ModeStage::Results { completed: true, .. })
            ),
        })
        .to_string()
    }

    /// The open duel's card page: `{"key": "...", "w": W, "h": H}`, the
    /// key changing whenever the faces do and empty while no duel is open,
    /// so the page rebuilds its copy of [`Self::card_page_rgba`] only then.
    pub fn card_page_info(&self) -> String {
        let Some(hands) = self.duel_hands() else {
            return r#"{"key":"","w":0,"h":0}"#.into();
        };
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for c in hands.iter().flatten() {
            for b in c.id.bytes().chain(c.sides) {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
            }
        }
        let (_, _, w, ph) = table_art::PAGE_RECT;
        json!({ "key": format!("{h:016x}"), "w": w, "h": ph }).to_string()
    }

    /// The open duel's card page as the play hosts write it into VRAM -
    /// both hands' faces and the back, with no portraits since this page
    /// has no disc - in RGBA8 over `table_art::PAGE_RECT`, row by row
    /// (`w * h * 4` bytes, a transparent texel at alpha `0`). Empty while
    /// no duel is open.
    pub fn card_page_rgba(&self) -> Vec<u8> {
        let Some(hands) = self.duel_hands() else {
            return Vec::new();
        };
        let (px, py, pw, ph) = table_art::PAGE_RECT;
        let (pw, ph) = (usize::from(pw), usize::from(ph));
        let mut out = vec![0u8; pw * ph * 4];
        for (x, y, w, h, texels) in table_art::compose_page(&hands, &|_| None) {
            let (x0, y0) = (usize::from(x - px), usize::from(y - py));
            for row in 0..usize::from(h) {
                for col in 0..usize::from(w) {
                    let (tx, ty) = (x0 + col, y0 + row);
                    if tx >= pw || ty >= ph {
                        continue;
                    }
                    let px = texel_rgba(texels[row * usize::from(w) + col]);
                    out[(ty * pw + tx) * 4..][..4].copy_from_slice(&px);
                }
            }
        }
        out
    }

    /// The seat's screen in the 320x240 stage, `{"prims": [[x0, y0, x1, y1,
    /// x2, y2, x3, y3, r, g, b, a, semi, abr, gouraud, tex]...], "labels":
    /// [[text, x, y, r, g, b, a, align, max_w]...]}`. Prims come in draw
    /// order, corners in `POLY_F4` order (top-left, top-right, bottom-left,
    /// bottom-right); `semi` marks a blended prim and `abr` its PSX blend
    /// mode (`0` half, `1` additive, `2` subtractive, `3` quarter add);
    /// `gouraud` is `null` for a flat fill or the four corners' `[r, g, b,
    /// a]` flattened in corner order (`r, g, b` above are then their
    /// average, for a reader that fills flat); `tex` is `null`, or for a
    /// card drawn off the duel's card page its page rectangle `[u0, v0, u1,
    /// v1]` (inclusive texels of [`Self::card_page_rgba`]) to stretch over
    /// the quad's bounds. A label's `y` is the top of its
    /// line, `align` is `0` left, `1` centre, `2` right of `x`, and `max_w`
    /// the width of its box (`0`: none) the page fits it to. Empty lists
    /// while no seat is open.
    pub fn frame_json(&self) -> String {
        let Some(seat) = self.seat.as_ref() else {
            return r#"{"prims":[],"labels":[]}"#.into();
        };
        let screen = seat_screen(seat);
        let prims = mode_screen_prims(&screen);
        let mut out_prims = Vec::with_capacity(prims.len());
        for i in order_primitives(&prims) {
            let q = match &prims[i] {
                ScreenPrim::Flat(q) => q,
                // A card face off the duel's page, which the page draws
                // from its copy of `card_page_rgba`.
                ScreenPrim::Textured(t) if t.tpage == table_art::tpage() => {
                    let [a, b, cc, d] = t.xy;
                    let [(u0, v0), _, _, (u1, v1)] = t.uv;
                    out_prims.push(json!([
                        a.0,
                        a.1,
                        b.0,
                        b.1,
                        cc.0,
                        cc.1,
                        d.0,
                        d.1,
                        128,
                        128,
                        128,
                        255,
                        0,
                        0,
                        null,
                        [u0, v0, u1, v1]
                    ]));
                    continue;
                }
                _ => continue,
            };
            let c = match q.gouraud {
                // A gradient reads as its average on the page's flat fill.
                Some(g) => {
                    let mut s = [0u32; 4];
                    for v in g {
                        for k in 0..4 {
                            s[k] += u32::from(v[k]);
                        }
                    }
                    s.map(|x| (x / 4) as u8)
                }
                None => q.color,
            };
            let [a, b, cc, d] = q.xy;
            out_prims.push(json!([
                a.0,
                a.1,
                b.0,
                b.1,
                cc.0,
                cc.1,
                d.0,
                d.1,
                c[0],
                c[1],
                c[2],
                c[3],
                u8::from(q.semi_transparent),
                q.abr_mode,
                q.gouraud.map(|g| g.concat()),
                null
            ]));
        }
        let labels: Vec<_> = mode_screen_labels(&screen)
            .into_iter()
            .map(|l| {
                let col = l.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                let align = match l.align {
                    Align::Left => 0,
                    Align::Centre => 1,
                    Align::Right => 2,
                };
                // The label's box width (`0`: none), which the page fits
                // the text to in its own font.
                let max_w = l.max_w.unwrap_or(0);
                json!([
                    l.text, l.x, l.y, col[0], col[1], col[2], col[3], align, max_w
                ])
            })
            .collect();
        json!({ "prims": out_prims, "labels": labels }).to_string()
    }
}

impl LegaiaModes {
    fn load_files(&mut self, files: &PackFiles) -> String {
        match ExpansionPack::from_files(files)
            .map_err(|e| format!("{e:#}"))
            .and_then(Rules::from_pack)
        {
            Ok(r) => {
                self.set_rules(r);
                String::new()
            }
            Err(e) => e,
        }
    }

    /// [`Self::load_pack_tree`] for a native caller (the tests).
    pub fn load_pack_files(&mut self, files: &PackFiles) -> String {
        self.load_files(files)
    }
}
