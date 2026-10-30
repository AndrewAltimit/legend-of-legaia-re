//! The Grid Duel's card page: the duel's card faces and the card back,
//! written into VRAM while a duel is seated so both hosts draw the cards as
//! textured primitives (`legaia_engine_modes::cards::table_art`).
//!
//! The page is written once per duel (and again if a rebuild of the
//! scene's VRAM drops it), inside the field VRAM step both hosts already run
//! and re-upload after. The words it covers are kept and put back when the
//! duel closes, so the scene's VRAM is what it was. Nothing here is retail
//! state: with no duel seated the step is a no-op.

use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::ModeView;
use legaia_engine_modes::cards::duel::DuelCard;
use legaia_engine_modes::cards::table_art::{self, BACK_SLOT, FACE_W, PAGE_RECT, PortraitPixels};
use legaia_engine_modes::poker::art as poker_art;
use legaia_expansion_format::refs::DiscRef;

use super::World;
use super::ambient::{read_rect, write_rect};

/// The back slot's row the page checks each frame to notice a VRAM rebuild.
const PROBE_ROW: u16 = 36;

impl World {
    /// The hands a seated duel shows: the session's, or the last the table
    /// kept once the session has gone to its results.
    pub fn duel_page_hands(&self) -> Option<[Vec<DuelCard>; 2]> {
        if self.mode != SceneMode::Expansion {
            return None;
        }
        let seat = self.expansion.mode_seat.as_deref()?;
        match &seat.stage {
            ModeStage::Playing { mode, .. } => match mode.view() {
                ModeView::Duel(v) => Some(v.hands),
                _ => None,
            },
            ModeStage::Results { .. } if seat.stage_frame.is_some() => {
                seat.world_stage.duel.last_view().map(|v| v.hands.clone())
            }
            _ => None,
        }
    }

    fn duel_page_key(&self, hands: &[Vec<DuelCard>; 2]) -> u64 {
        // FNV-1a over what the faces show.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |b: &[u8]| {
            for &x in b {
                h ^= u64::from(x);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for hand in hands {
            for c in hand {
                eat(c.id.as_bytes());
                eat(&c.sides);
                eat(&[c.rarity as u8, c.element.map_or(0xFF, |e| e as u8)]);
            }
        }
        eat(self.expansion.cards.portraits.pack_id.as_bytes());
        eat(&(self.expansion.cards.portraits.by_monster.len() as u32).to_le_bytes());
        h
    }

    /// What the table page shows this frame: a seated duel's hands, or an
    /// in-world poker table's visible cards.
    fn table_page_content(&self) -> Option<PageContent> {
        if let Some(hands) = self.duel_page_hands() {
            return Some(PageContent::Duel(hands));
        }
        if self.mode != SceneMode::Expansion {
            return None;
        }
        let seat = self.expansion.mode_seat.as_deref()?;
        let poker = seat.stage_frame.as_ref()?.poker.as_ref()?;
        Some(PageContent::Poker(poker.visible.clone()))
    }

    /// Write the seated table's card page into `vram` (a duel's faces, or a
    /// poker table's visible cards), or put the covered words back once no
    /// table is seated. Returns whether `vram` changed.
    pub fn step_duel_card_page(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let (px, py, pw, ph) = PAGE_RECT;
        let Some(content) = self.table_page_content() else {
            let page = &mut self.expansion.cards.duel_page;
            page.key = None;
            let Some((scene, words)) = page.saved.take() else {
                return false;
            };
            if scene != self.active_scene_label {
                // The scene changed under the table: its rebuild already
                // holds the scene's own words.
                return false;
            }
            write_rect(vram, px, py, pw, &words);
            return true;
        };
        // The probe: one row of the back, which every page draws.
        let (back, back_at, back_w) = match &content {
            PageContent::Duel(_) => {
                let (u, v) = table_art::slot_uv(BACK_SLOT);
                (table_art::compose_back(), (u, v), FACE_W)
            }
            PageContent::Poker(_) => {
                let r = poker_art::slot_rect(poker_art::BACK_SLOT);
                (poker_art::compose_back(), (r[0], r[1]), u16::from(r[2]))
            }
        };
        let key = match &content {
            PageContent::Duel(hands) => self.duel_page_key(hands),
            PageContent::Poker(v) => {
                let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
                for b in format!("{v:?}").bytes() {
                    h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
                }
                h
            }
        };
        let probe_at = (
            px + u16::from(back_at.0),
            py + u16::from(back_at.1) + PROBE_ROW,
        );
        let w = usize::from(back_w);
        let want = back[usize::from(PROBE_ROW) * w..usize::from(PROBE_ROW + 1) * w].to_vec();
        let intact = read_rect(vram, probe_at.0, probe_at.1, back_w, 1) == want;
        let page = &self.expansion.cards.duel_page;
        if page.key == Some(key) && intact {
            return false;
        }
        if !intact {
            // The first write, or the scene's VRAM was rebuilt under the
            // page: keep the words it covers.
            let words = read_rect(vram, px, py, pw, ph);
            self.expansion.cards.duel_page.saved = Some((self.active_scene_label.clone(), words));
        }
        let rects = match &content {
            PageContent::Duel(hands) => {
                let pack = self.expansion.pack.clone();
                let portraits = &self.expansion.cards.portraits;
                let portrait = |id: &str| -> Option<PortraitPixels<'_>> {
                    let art = &pack.as_ref()?.cards.cards.iter().find(|c| c.id == id)?.art;
                    match art {
                        Some(DiscRef::Monster { id }) => {
                            portraits.get(*id).map(|p| PortraitPixels {
                                w: p.w,
                                h: p.h,
                                rgba: &p.rgba,
                            })
                        }
                        _ => None,
                    }
                };
                table_art::compose_page(hands, &portrait)
            }
            PageContent::Poker(v) => poker_art::compose_page(v),
        };
        for (x, y, w, _h, texels) in rects {
            write_rect(vram, x, y, w, &texels);
        }
        self.expansion.cards.duel_page.key = Some(key);
        true
    }
}

/// What the table page holds.
enum PageContent {
    Duel([Vec<DuelCard>; 2]),
    Poker(poker_art::VisibleCards),
}
