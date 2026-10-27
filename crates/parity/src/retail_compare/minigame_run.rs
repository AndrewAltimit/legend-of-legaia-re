//! Minigame states (`OTHER MODE`, game mode `0x19`): which minigame a capture
//! runs, how far the engine can be seeded into it, and the channels it is
//! scored on.
//!
//! The minigame is named by the mode-24 door warp's sub-id `_DAT_8007BA34`
//! (field-VM op `0x3E`, `0x801E07B8`), which selects the overlay
//! ([`legaia_engine_mode::minigame_entry::MinigameSubId`]); the scene label
//! stays the departure scene's. The seed lands the save in that scene and
//! takes the same warp (`World::request_minigame_warp`), so the engine enters
//! the session a cabinet installs.
//!
//! Only a session whose captured phase the warp itself reaches is seeded: a
//! phase that needs played input past the entry carries `rand()` history (a
//! slot machine's reel stops) the seed does not replay. Today that is the
//! slot machine at rest - reel state word `DAT_801D3C84 = 1`, the attract
//! state the init (`0`) advances to.

use super::*;
use legaia_engine_mode::minigame_entry::MinigameSubId;

/// The mode-24 door warp's sub-id (`_DAT_8007BA34`, `sh` at `0x801E07B8`).
const MINIGAME_SUB_ID: u32 = 0x8007_BA34;
/// The slot machine's reel state word (`DAT_801D3C84`, PROT 0975).
const SLOT_STATE: u32 = 0x801D_3C84;
/// The slot machine's balance (`DAT_801D4114`), assigned from the coin bank
/// at entry.
const SLOT_BALANCE: u32 = 0x801D_4114;
/// The casino coin bank (`_DAT_800845A4`).
const COIN_BANK: u32 = 0x8008_45A4;
/// The battle-return flag the slot init tests (`_DAT_8007B8B8`).
const BATTLE_RETURN_FLAG: u32 = 0x8007_B8B8;
/// The slot machine's attract state.
const SLOT_IDLE: i32 = 1;
/// The tick the seed takes the warp on, after the scene entry settles.
const WARP_TICK: u64 = 40;
/// Ticks the entry may take to reach the captured phase.
const ENTRY_DEADLINE: u64 = 900;
/// The image child's frame, past the warp: the captures sample the attract
/// state 300 vsyncs after the field was left.
const FRAME_TICKS_PAST_WARP: u64 = 300;

/// The minigame a capture runs and its scored words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMinigame {
    /// `_DAT_8007BA34`.
    pub sub_id: i16,
    /// The slot machine's state word and balance, when the slot overlay is
    /// the one resident.
    pub slot: Option<(i32, i32)>,
    /// `_DAT_800845A4`.
    pub coin_bank: u32,
    /// The battle-return flag `_DAT_8007B8B8` is up: the overlay was entered
    /// through the door path, which assigns the balance from the bank.
    pub door_entry: bool,
}

impl RetailMinigame {
    pub(super) fn from_ram(ram: &[u8]) -> Self {
        let sub_id = game_anchors::i16_at(ram, MINIGAME_SUB_ID);
        let slot = (u8::try_from(sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
            == Some(MinigameSubId::SlotMachine))
        .then(|| {
            (
                game_anchors::u32_at(ram, SLOT_STATE) as i32,
                game_anchors::u32_at(ram, SLOT_BALANCE) as i32,
            )
        });
        Self {
            sub_id,
            slot,
            coin_bank: game_anchors::u32_at(ram, COIN_BANK),
            door_entry: game_anchors::u32_at(ram, BATTLE_RETURN_FLAG) != 0,
        }
    }

    fn minigame(&self) -> Option<MinigameSubId> {
        u8::try_from(self.sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
    }

    /// Why the state is not seeded, or `None` when it is.
    pub fn unseeded_reason(&self) -> Option<String> {
        match (self.minigame(), self.slot) {
            // The door path's init assigns the balance from the coin bank
            // (`FUN_801CEC94`); with the battle-return flag clear it is the
            // 70-coin dev fallback instead, a launch outside the casino door
            // the warp does not take.
            (Some(MinigameSubId::SlotMachine), Some(_)) if !self.door_entry => Some(
                "slot machine launched outside the casino door (battle-return flag \
                 _DAT_8007B8B8 clear, the 70-coin dev balance)"
                    .to_string(),
            ),
            // Only a spin or a payout moves the balance off the bank: a
            // machine at rest off it has been played.
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance)))
                if i64::from(balance) == i64::from(self.coin_bank) =>
            {
                None
            }
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance))) => Some(format!(
                "slot machine at rest after play (balance {balance}, coin bank {}): the reel \
                 positions and the balance are rand() history the seed does not replay",
                self.coin_bank
            )),
            (Some(MinigameSubId::SlotMachine), Some((state, _))) => Some(format!(
                "slot machine in reel state {state:#X}: past the attract state the reel stops \
                 and feature rolls are rand() history the seed does not replay"
            )),
            (Some(m), _) => Some(format!(
                "no minigame session seeding path for {m:?} (sub-id {})",
                self.sub_id
            )),
            (None, _) => Some(format!("minigame sub-id {} names no overlay", self.sub_id)),
        }
    }
}

/// The engine side of a seeded minigame state.
struct EngineMinigame {
    mode: SceneMode,
    scene: String,
    slot: Option<(i32, i32)>,
    coin_bank: u32,
    /// The tick the captured phase was reached on, past the warp.
    reached: Option<u64>,
}

fn run_engine_minigame(extracted: &Path, retail: &RetailObs) -> Result<EngineMinigame> {
    let mg = retail.minigame.context("no minigame observables")?;
    let sub = mg.minigame().context("sub-id names no overlay")?;
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).context("open boot session")?;
    crate::retail_compare::pin_seed_retail(&mut session.host.world.toggles, false);
    let save = retail
        .seed_save()
        .context("retail state has no liftable save window")?;
    let _landing = session.resume_save(save, &retail.scene, &FieldLiveOpts::default());
    for _ in 0..WARP_TICK {
        session.tick()?;
    }
    session.host.world.request_minigame_warp(sub.sub_id());
    let want = mg.slot.map(|(state, _)| state);
    let mut reached = None;
    for t in 0..ENTRY_DEADLINE {
        session.tick()?;
        let state = session
            .host
            .world
            .minigames
            .slot_machine
            .as_ref()
            .map(|m| m.retail_state());
        if session.host.world.mode == SceneMode::SlotMachine && state == want {
            reached = Some(t);
            break;
        }
    }
    let w = &session.host.world;
    Ok(EngineMinigame {
        mode: w.mode,
        scene: session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        slot: w
            .minigames
            .slot_machine
            .as_ref()
            .map(|m| (m.retail_state(), m.balance())),
        coin_bank: w.minigames.casino_coins,
        reached,
    })
}

/// Seed a minigame state and score it: `mode`, `scene`, `minigame` (the
/// session's own words) and `image`.
pub(super) fn run_minigame(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    report: &mut StateReport,
) {
    let Some(mg) = retail.minigame else {
        report.unseeded = StateClass::Minigame.unseeded_reason().to_string();
        return;
    };
    if let Some(why) = mg.unseeded_reason() {
        report.unseeded = why;
        return;
    }
    let engine = match run_engine_minigame(opts.extracted, retail) {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return;
        }
    };
    let mut ch = BTreeMap::new();
    let mode_ok = engine.mode == SceneMode::SlotMachine;
    ch.insert("mode".to_string(), f64::from(u8::from(mode_ok)));
    report.detail.insert(
        "mode".into(),
        format!("retail 0x19 sub-id {}; engine {:?}", mg.sub_id, engine.mode),
    );
    ch.insert(
        "scene".to_string(),
        f64::from(u8::from(engine.scene == retail.scene)),
    );
    let words = [
        engine.slot.map(|s| s.0) == mg.slot.map(|s| s.0),
        engine.slot.map(|s| s.1) == mg.slot.map(|s| s.1),
        engine.coin_bank == mg.coin_bank,
    ];
    ch.insert(
        "minigame".to_string(),
        round3(words.iter().filter(|&&w| w).count() as f64 / words.len() as f64),
    );
    report.detail.insert(
        "minigame".into(),
        format!(
            "slot state/balance retail {:?} engine {:?}; coin bank retail {} engine {}; phase \
             reached {:?} ticks past the warp",
            mg.slot, engine.slot, mg.coin_bank, engine.coin_bank, engine.reached
        ),
    );
    if let (Some(exe), Some(rf), Some(save)) =
        (opts.engine_exe, &retail.frame, retail.seed_save().as_ref())
    {
        let warp = WARP_TICK;
        let extra = vec!["--key-script".to_string(), format!("{warp}:O")];
        match crate::retail_compare_image::engine_frame_with(
            exe,
            opts.extracted,
            &retail.scene,
            None,
            &extra,
            &[],
            warp + FRAME_TICKS_PAST_WARP,
            opts.out_dir,
            &entry.label,
            crate::retail_compare_image::FrameEntry::Resume(save),
        ) {
            Ok(ef) => {
                if let Some(dir) = opts.out_dir {
                    let _ = crate::retail_compare_image::write_side_by_side(
                        &dir.join(format!("{}.png", entry.label)),
                        rf,
                        &ef,
                    );
                }
                let score = crate::retail_compare_image::score(rf, &ef);
                ch.insert("image".into(), round3(score.within));
                report.detail.insert(
                    "image".into(),
                    format!(
                        "mae={:.1} within={:.3} ({})",
                        score.mae, score.within, score.note
                    ),
                );
                report.image = Some(score);
            }
            Err(e) => {
                report
                    .detail
                    .insert("image".into(), format!("engine frame failed: {e:#}"));
            }
        }
    }
    report.score = state_score(&ch);
    report.channels = ch;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(state: i32, balance: i32, bank: u32, door: bool) -> RetailMinigame {
        RetailMinigame {
            sub_id: 3,
            slot: Some((state, balance)),
            coin_bank: bank,
            door_entry: door,
        }
    }

    #[test]
    fn only_a_fresh_door_entry_slot_machine_is_seeded() {
        assert_eq!(slot(1, 9_999_999, 9_999_999, true).unseeded_reason(), None);
        // A decided spin, a played machine, a dev launch.
        assert!(
            slot(4, 9_999_996, 9_999_999, true)
                .unseeded_reason()
                .is_some()
        );
        assert!(slot(1, 120, 100, true).unseeded_reason().is_some());
        assert!(slot(1, 70, 0, false).unseeded_reason().is_some());
        // Another minigame has no path yet.
        let dance = RetailMinigame {
            sub_id: 6,
            slot: None,
            coin_bank: 0,
            door_entry: true,
        };
        assert!(dance.unseeded_reason().unwrap().contains("Dance"));
    }
}
