//! The play page's resume and New Game entries: the browser half of
//! [`legaia_engine_core::resume`], paired with the native
//! `BootSession::resume_save` / `BootSession::start_new_game`.
//!
//! The page used to choreograph both in `play.html` callbacks - enter the
//! save's scene if its own picker list carried it, otherwise fall through to
//! a New Game; seed and enter `opdeene`, then `town01` if that threw - and
//! every callback (in-canvas Load, title Continue, card / LGSF import, the
//! post-wipe title) spelled the sequence out again. Now a Load or an import
//! only **parks** the save here ([`LegaiaRuntime::park_loaded_save`]), and
//! the page makes one call:
//!
//! - [`LegaiaRuntime::play_resume_save`] lands the parked save through
//!   [`legaia_engine_core::resume::land_save`] (the saved scene, else the
//!   running scene, else the opening town - never a New Game);
//! - [`LegaiaRuntime::play_new_game`] seeds the New Game slate and enters the
//!   opening through [`legaia_engine_core::resume::enter_new_game`].
//!
//! Both return the landing as JSON; the page rebuilds its render state when
//! `entered` is set and otherwise leaves the scene it has up.

use legaia_engine_core::resume::{CardLoadHost, ResumeLanding, enter_new_game, resume_card_load};
use legaia_save::SaveFile;
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// A loaded save waiting for its landing: the save itself and the scene it
/// names (empty when it names none).
#[derive(Debug, Clone)]
pub(crate) struct ParkedResume {
    pub save: SaveFile,
    pub scene: String,
}

impl LegaiaRuntime {
    /// Park a loaded save for [`Self::play_resume_save`], which lands it
    /// through the shared card-load kernel. Every Load and import on this
    /// host parks through here - including a save that names no scene, which
    /// then lands on the running scene rather than being dropped.
    ///
    /// The park leaves the world alone. It used to apply the whole save at
    /// once, ahead of the landing's entry, and a party record raised its
    /// actor slot off a field: a four-member save landed on `uru` from the
    /// title drew the scene-pack meshes pre-bound to slots 1..3. The kernel
    /// seeds only the story flags ahead of the entry and the whole save after
    /// it, as the native window does.
    pub(crate) fn park_loaded_save(&mut self, save: SaveFile, scene: &str) {
        self.pending_card_resume = Some(ParkedResume {
            save,
            scene: scene.to_string(),
        });
    }

    /// The CDNAME label of the scene the host has loaded, if any.
    fn running_scene(&self) -> Option<String> {
        self.scene_host
            .host()
            .and_then(|h| h.scene.as_ref())
            .map(|s| s.name.clone())
    }

    /// Testable body of [`Self::play_resume_save`]. `None` when nothing is
    /// parked.
    pub(crate) fn resume_parked_save(&mut self) -> Option<ResumeLanding> {
        let parked = self.pending_card_resume.take()?;
        // A picked resume is never the next leg of an opening chain the
        // player was in: abandon it the way a scene pick does.
        self.play_abandon_opening_chain();
        // The order (story flags, landing, whole save) is the shared
        // kernel's - `resume_card_load`, which the native
        // `BootSession::resume_save` runs too.
        let landing = resume_card_load(&mut PageCardLoad(self), parked.save, &parked.scene);
        crate::console_log(&format!(
            "resume: landed {} ({:?})",
            landing.kind(),
            landing.scene()
        ));
        Some(landing)
    }

    /// Testable body of [`Self::play_new_game`].
    pub(crate) fn start_new_game(&mut self) -> Option<&'static str> {
        self.pending_card_resume = None;
        self.play_abandon_opening_chain();
        self.begin_new_game();
        enter_new_game(|scene| self.enter_field_core(scene, false).map(|_| ()))
    }

    fn landing_json(&self, kind: &str, scene: Option<&str>, entered: bool) -> String {
        let state: serde_json::Value =
            serde_json::from_str(&self.state_json()).unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "landing": kind,
            "scene": scene.unwrap_or(""),
            "entered": entered,
            "state": state,
        })
        .to_string()
    }
}

/// The play page's half of a card load
/// ([`legaia_engine_core::resume::resume_card_load`]).
struct PageCardLoad<'a>(&'a mut LegaiaRuntime);

impl CardLoadHost for PageCardLoad<'_> {
    fn card_load_world(&mut self) -> &mut legaia_engine_core::world::World {
        self.0.world_mut()
    }

    fn card_load_running_scene(&self) -> Option<String> {
        self.0.running_scene()
    }

    fn card_load_enter(
        &mut self,
        scene: &str,
        save: &SaveFile,
        save_scene: &str,
    ) -> Result<(), String> {
        // The saved scene is entered at the save's own position, as retail's
        // card load seats it (`SceneHost::arm_resume_seat`); the entry skips
        // the picker's free-roam story baseline (a resume is not a visit).
        let rt = &mut *self.0;
        let armed = rt
            .scene_host
            .host_mut()
            .is_some_and(|h| h.arm_resume_seat(save, save_scene, scene));
        let entered = rt.enter_field_core(scene, true).map(|_| ());
        if entered.is_err()
            && armed
            && let Some(h) = rt.scene_host.host_mut()
        {
            h.disarm_entry_seat();
        }
        entered
    }

    fn card_load_hydrated(&mut self) {
        if let Some(h) = self.0.scene_host.host_mut() {
            h.refresh_party_battle_inputs();
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Land the save a Load or an import parked (the browser twin of the
    /// native `BootSession::resume_save`). Returns
    /// `{"landing": "saved"|"current"|"opening"|"nowhere", "scene", "entered",
    /// "state"}`; `state` is [`Self::state_json`] after the landing. The page
    /// rebuilds its render state when `entered` is `true`. Throws when
    /// nothing is parked.
    pub fn play_resume_save(&mut self) -> Result<String, JsValue> {
        let landing = self
            .resume_parked_save()
            .ok_or_else(|| JsValue::from_str("play_resume_save: no loaded save is waiting"))?;
        Ok(self.landing_json(landing.kind(), landing.scene(), landing.entered_scene()))
    }

    /// Start a New Game (the browser twin of the native
    /// `BootSession::start_new_game`): the seeded slate
    /// ([`Self::begin_new_game`], which also stops the title theme), then the
    /// opening - `opdeene`, else `town01`. Returns the same JSON shape as
    /// [`Self::play_resume_save`] with `landing` = `"new_game"`. Throws when
    /// neither opening scene would enter.
    pub fn play_new_game(&mut self) -> Result<String, JsValue> {
        let scene = self
            .start_new_game()
            .ok_or_else(|| JsValue::from_str("play_new_game: no opening scene would enter"))?;
        Ok(self.landing_json("new_game", Some(scene), true))
    }

    /// `true` when a card **Load** committed and its save is parked for
    /// [`Self::play_resume_save`] - the page then closes the menu and lands
    /// it. Consumes the menu's parked label. A save that names no scene
    /// counts too (it lands on the running scene); the label-only
    /// [`Self::play_menu_take_load_scene`] answers `""` for it, which is why
    /// the page used to leave such a Load unlanded.
    pub fn play_menu_take_load(&mut self) -> bool {
        let label = self.play_menu_take_load_scene();
        !label.is_empty() || self.pending_card_resume.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::SaveResume;

    fn disc_runtime() -> Option<LegaiaRuntime> {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return None;
        };
        let Ok(bytes) = std::fs::read(&disc) else {
            eprintln!("[skip] disc unreadable (disc-gated)");
            return None;
        };
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes, String::new()).expect("load_disc");
        Some(rt)
    }

    fn lgsf(rt: &mut LegaiaRuntime, money: i32, scene: &str) -> Vec<u8> {
        rt.world_mut().party.money = money;
        rt.world_mut().save_full().write_with_resume(&SaveResume {
            scene: scene.into(),
            location: String::new(),
        })
    }

    #[test]
    fn nothing_parked_is_no_landing() {
        let mut rt = LegaiaRuntime::new();
        assert!(rt.resume_parked_save().is_none());
    }

    /// With no disc and no scene the landing is `Nowhere`, and the save still
    /// applies - the resume never discards the save the player picked.
    #[test]
    fn a_resume_with_nothing_enterable_still_applies_the_save() {
        let mut rt = LegaiaRuntime::new();
        rt.world_mut().party.money = 77;
        let sf = rt.world_mut().save_full();
        rt.world_mut().party.money = 1;
        rt.park_loaded_save(sf, "town01");
        rt.world_mut().party.money = 1;
        assert_eq!(rt.resume_parked_save(), Some(ResumeLanding::Nowhere));
        assert_eq!(rt.world_mut().party.money, 77);
        assert!(rt.pending_card_resume.is_none(), "the park is consumed");
    }

    /// The page's three resume shapes, disc-gated (`enter_field` needs the
    /// disc): the saved scene is entered even when it is the one already
    /// running (a Load of a save written in the open scene), a scene the host
    /// cannot enter lands on the running scene with the save applied, and
    /// with nothing running it lands in the opening town - never in the
    /// New Game the page used to fall through to.
    #[test]
    fn resume_lands_saved_then_current_then_opening_town() {
        let Some(mut rt) = disc_runtime() else { return };

        // Nothing running + unenterable scene -> the opening town, save kept.
        let bytes = lgsf(&mut rt, 4321, "no_such_scene");
        rt.world_mut().party.money = 1;
        rt.import_save_core(&bytes).expect("import");
        assert_eq!(rt.resume_parked_save(), Some(ResumeLanding::OpeningTown));
        assert_eq!(rt.running_scene().as_deref(), Some("town01"));
        assert_eq!(rt.world_mut().party.money, 4321, "not a New Game");

        // Save written in the open scene -> re-entered.
        let bytes = lgsf(&mut rt, 555, "town01");
        rt.world_mut().party.money = 1;
        rt.import_save_core(&bytes).expect("import");
        assert_eq!(
            rt.resume_parked_save(),
            Some(ResumeLanding::SavedScene("town01".into()))
        );
        assert_eq!(rt.world_mut().party.money, 555);
        assert!(!rt.world_mut().field_vm.free_roam_staging);

        // Unenterable scene with a scene running -> loads over it.
        let bytes = lgsf(&mut rt, 666, "no_such_scene");
        rt.world_mut().party.money = 1;
        rt.import_save_core(&bytes).expect("import");
        assert_eq!(
            rt.resume_parked_save(),
            Some(ResumeLanding::CurrentScene("town01".into()))
        );
        assert_eq!(rt.world_mut().party.money, 666);

        // A different enterable scene -> entered.
        let bytes = lgsf(&mut rt, 888, "town0b");
        rt.import_save_core(&bytes).expect("import");
        assert_eq!(
            rt.resume_parked_save(),
            Some(ResumeLanding::SavedScene("town0b".into()))
        );
        assert_eq!(rt.running_scene().as_deref(), Some("town0b"));
        assert_eq!(rt.world_mut().party.money, 888);
        eprintln!("[ok] page resume: opening town / saved / current / saved");
    }

    /// A four-member card load onto a field raises no actor but the
    /// player's - the page twin of the native
    /// `resume_landing::a_four_member_resume_on_uru_raises_only_the_player_and_keeps_the_ground`.
    /// Both from a cold runtime (a title Continue) and from a field already
    /// running (an in-canvas Load), the slots `uru` pre-binds to scene-pack
    /// meshes stay idle.
    #[test]
    fn a_four_member_resume_on_uru_raises_only_the_player() {
        let Some(mut rt) = disc_runtime() else { return };
        let save = SaveFile {
            party: legaia_save::Party::zeroed(4),
            ..Default::default()
        };
        for pass in ["cold", "over town01"] {
            if pass != "cold" {
                rt.enter_field_core("town01", false).expect("enter town01");
            }
            rt.park_loaded_save(save.clone(), "uru");
            assert_eq!(
                rt.resume_parked_save(),
                Some(ResumeLanding::SavedScene("uru".into()))
            );
            let world = rt.world_mut();
            assert_eq!(world.party.roster.members.len(), 4);
            let player = usize::from(world.player_actor_slot.expect("a field player"));
            assert!(
                (1..4).any(|s| world.actors[s].tmd_binding.is_some()),
                "uru pre-binds scene-pack meshes onto slots 1..3"
            );
            for slot in 0..4 {
                assert_eq!(
                    world.actor_slot_drawn(slot, false),
                    slot == player,
                    "{pass}: actor {slot} drawn after the page resume"
                );
            }
        }
        eprintln!("[ran] page four-member resume on uru: player-only actors");
    }

    /// A New Game after a resumed save takes the seeded slate and the
    /// prologue - the post-wipe title's New Game goes through this too.
    #[test]
    fn new_game_after_a_resume_is_a_fresh_slate_in_the_prologue() {
        let Some(mut rt) = disc_runtime() else { return };
        let bytes = lgsf(&mut rt, 4321, "town01");
        rt.import_save_core(&bytes).expect("import");
        rt.resume_parked_save().expect("landing");
        assert_eq!(
            rt.start_new_game(),
            Some(legaia_asset::new_game::OPENING_CUTSCENE_SCENE)
        );
        assert_eq!(
            rt.world_mut().party.money,
            legaia_engine_core::world::NEW_GAME_STARTING_GOLD
        );
        assert_eq!(
            rt.running_scene().as_deref(),
            Some(legaia_asset::new_game::OPENING_CUTSCENE_SCENE)
        );
        eprintln!("[ok] page new game after a resume");
    }
}
