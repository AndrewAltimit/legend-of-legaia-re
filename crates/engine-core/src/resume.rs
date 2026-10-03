//! Where a loaded save resumes, and where a New Game starts: the scene-entry
//! choreography every host runs, decided once.
//!
//! Retail resumes a save in the scene it was written in (the save carries
//! the scene label and banner name - [`legaia_save::SaveResume`]) and starts
//! a New Game in the prologue cutscene scene, which hands off to Rim Elm
//! (`docs/subsystems/boot.md`). Both are a *sequence* of scene entries with a
//! fallback at each step, and each host used to spell the sequence out for
//! itself: the native window in `BootSession`, the browser play page in its
//! page callbacks. The two drifted - the page gated the saved scene on its
//! own scene-picker list and, when a title Continue named a scene that list
//! did not carry, fell through to a New Game over the save it had just
//! loaded, while the native window loaded the same save onto the scene
//! already running.
//!
//! The functions here own the order and the fallbacks. A host supplies only
//! what it owns - its world, its running scene, and how to enter a scene -
//! through [`CardLoadHost`], and [`resume_card_load`] runs the card load:
//! the saved story flags ahead of the entry (retail's card load fills the
//! game-state window before the field init), the landing, then the whole
//! save ([`crate::world::World::load_full`] runs *after* the entry, because scene
//! entry resets per-scene world state).
//!
//! # Resume order
//!
//! 1. The save's own scene, when it names one and the host can enter it.
//! 2. The scene already running, with the save loaded over it.
//! 3. The opening town ([`legaia_asset::new_game::OPENING_SCENE`]), when
//!    nothing is running - a cold boot's title Continue on a host that did
//!    not pre-boot a scene.
//!
//! A resume never becomes a New Game: the save the player picked is applied
//! whatever the landing. A host's scene-picker list is not a resume gate -
//! "can this host enter the label" is answered by entering it.
//!
//! # New Game order
//!
//! The prologue cutscene scene, then the opening town when the cutscene
//! scene will not enter ([`NEW_GAME_SCENES`]).

use legaia_asset::new_game::{OPENING_CUTSCENE_SCENE, OPENING_SCENE};

/// The scenes a New Game tries, in order: the prologue cutscene chain's
/// first leg, then Rim Elm when that leg cannot be entered.
pub const NEW_GAME_SCENES: [&str; 2] = [OPENING_CUTSCENE_SCENE, OPENING_SCENE];

/// Where [`land_save`] put the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeLanding {
    /// Entered the scene the save was written in.
    SavedScene(String),
    /// The save named no scene this host could enter; it loads over the
    /// scene already running, which was not re-entered.
    CurrentScene(String),
    /// Neither the saved scene nor a running scene: entered the opening town.
    OpeningTown,
    /// Nothing could be entered and nothing was running. The save still
    /// loads into the world.
    Nowhere,
}

impl ResumeLanding {
    /// `true` when the landing entered a scene - the host's render-side
    /// scene state is stale and must be rebuilt.
    pub fn entered_scene(&self) -> bool {
        matches!(self, Self::SavedScene(_) | Self::OpeningTown)
    }

    /// The scene the player stands in after the landing (`None` for
    /// [`Self::Nowhere`]).
    pub fn scene(&self) -> Option<&str> {
        match self {
            Self::SavedScene(s) | Self::CurrentScene(s) => Some(s),
            Self::OpeningTown => Some(OPENING_SCENE),
            Self::Nowhere => None,
        }
    }

    /// Short stable name for a host's diagnostics / JSON bridge.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SavedScene(_) => "saved",
            Self::CurrentScene(_) => "current",
            Self::OpeningTown => "opening",
            Self::Nowhere => "nowhere",
        }
    }
}

/// Decide - and perform, through `enter` - where a loaded save resumes.
///
/// `save_scene` is the save's resume label (empty for a save that carries
/// none); `current_scene` is the scene the host has running, if any.
/// `enter` enters one scene and reports failure; it is called at most twice
/// (the saved scene, then the opening town). The caller applies the save
/// after this returns, whatever the landing.
pub fn land_save<E: std::fmt::Display>(
    save_scene: &str,
    current_scene: Option<&str>,
    mut enter: impl FnMut(&str) -> Result<(), E>,
) -> ResumeLanding {
    if !save_scene.is_empty() {
        match enter(save_scene) {
            Ok(()) => return ResumeLanding::SavedScene(save_scene.to_string()),
            Err(e) => log::warn!(
                "resume: the save names scene '{save_scene}' but entering it failed ({e}); \
                 falling back"
            ),
        }
    }
    if let Some(cur) = current_scene.filter(|s| !s.is_empty()) {
        return ResumeLanding::CurrentScene(cur.to_string());
    }
    match enter(OPENING_SCENE) {
        Ok(()) => ResumeLanding::OpeningTown,
        Err(e) => {
            log::warn!("resume: no scene running and '{OPENING_SCENE}' failed to enter ({e})");
            ResumeLanding::Nowhere
        }
    }
}

/// The seat a resume's entry into `scene` takes from `save`: the field
/// position the save was written at, when `scene` is the save's own scene.
///
/// Retail seats a card load from the save's position snapshot (the MAN
/// loader's `_DAT_8007B8C0` arm, [`crate::scene::SceneHost::arm_resume_seat`]).
/// Every other landing - the running scene, the opening-town fallback - is
/// not where the position was taken, so it gets no seat and enters as that
/// scene always does. `None` too for a save that names no position.
pub fn saved_entry_seat(
    save: &legaia_save::SaveFile,
    save_scene: &str,
    scene: &str,
) -> Option<(i16, i16)> {
    if save_scene.is_empty() || scene != save_scene {
        return None;
    }
    save.ext_v2.field_position
}

/// What a host lends [`resume_card_load`]: its world, the scene it has
/// running, and how it enters one scene for a resume.
pub trait CardLoadHost {
    /// The live world the save hydrates.
    fn card_load_world(&mut self) -> &mut crate::world::World;
    /// The CDNAME label of the scene the host has running, if any.
    fn card_load_running_scene(&self) -> Option<String>;
    /// Enter `scene` for a resume of `save` (whose own resume label is
    /// `save_scene`): the host arms the save's seat
    /// ([`crate::scene::SceneHost::arm_resume_seat`]) and runs its scene
    /// entry. The kernel hydrates the world afterwards - an implementation
    /// does not apply the save itself.
    fn card_load_enter(
        &mut self,
        scene: &str,
        save: &legaia_save::SaveFile,
        save_scene: &str,
    ) -> Result<(), String>;
    /// Host-side state the hydrated save re-prices (the arts input's
    /// equipment inputs, `SceneHost::refresh_party_battle_inputs`).
    fn card_load_hydrated(&mut self);
}

/// A card load, the order both hosts run it in: the saved **story flags**
/// first, then the landing ([`land_save`]), then the **whole save** over the
/// landing.
///
/// Retail's card load copies the save block over the live game-state window
/// before the field init runs, so the landing scene's entry scripts and
/// bind-time prologues read the saved story (a `rikuroa` loaded after the
/// Genesis tree's revival, flag `0x142`, does not re-arm the withered tree's
/// morph lanes). Only the flags go in ahead
/// ([`crate::world::World::load_story_flags`]): the party records raise actor
/// slots off a field ([`crate::world::World::load_party`]), and a slot raised
/// before the entry survives it - a four-member save landed on `uru` drew
/// the scene-pack meshes pre-bound to slots 1..3. The full
/// [`crate::world::World::load_full`] follows the landing, whatever the landing.
pub fn resume_card_load(
    host: &mut impl CardLoadHost,
    save: legaia_save::SaveFile,
    save_scene: &str,
) -> ResumeLanding {
    host.card_load_world().load_story_flags(&save);
    let current = host.card_load_running_scene();
    let landing = land_save(save_scene, current.as_deref(), |scene| {
        host.card_load_enter(scene, &save, save_scene)
    });
    host.card_load_world().load_full(save);
    host.card_load_hydrated();
    landing
}

/// Enter the New Game's first scene through `enter`, trying
/// [`NEW_GAME_SCENES`] in order. Returns the scene entered, or `None` when
/// none would enter. The caller has already reset and seeded the world
/// ([`crate::world::World::begin_new_game`] + the starting-party seed).
pub fn enter_new_game<E: std::fmt::Display>(
    mut enter: impl FnMut(&str) -> Result<(), E>,
) -> Option<&'static str> {
    for scene in NEW_GAME_SCENES {
        match enter(scene) {
            Ok(()) => return Some(scene),
            Err(e) => log::warn!("new game: entering '{scene}' failed ({e})"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter_only(ok: &'static [&'static str]) -> impl FnMut(&str) -> Result<(), String> {
        move |s: &str| {
            if ok.contains(&s) {
                Ok(())
            } else {
                Err(format!("no scene {s}"))
            }
        }
    }

    /// A host that records what the world held when the landing entered.
    struct ProbeHost {
        world: crate::world::World,
        running: Option<String>,
        enterable: &'static [&'static str],
        /// `(flag 0x142 set, roster length, money)` at each entry.
        seen_at_entry: Vec<(bool, usize, i32)>,
        hydrated: bool,
    }

    impl CardLoadHost for ProbeHost {
        fn card_load_world(&mut self) -> &mut crate::world::World {
            &mut self.world
        }
        fn card_load_running_scene(&self) -> Option<String> {
            self.running.clone()
        }
        fn card_load_enter(
            &mut self,
            scene: &str,
            _save: &legaia_save::SaveFile,
            _save_scene: &str,
        ) -> Result<(), String> {
            self.seen_at_entry.push((
                self.world.system_flag_test(0x142),
                self.world.party.roster.members.len(),
                self.world.party.money,
            ));
            self.enterable
                .contains(&scene)
                .then_some(())
                .ok_or_else(|| format!("no scene {scene}"))
        }
        fn card_load_hydrated(&mut self) {
            self.hydrated = true;
        }
    }

    /// The card load seeds the story flags ahead of the entry and nothing
    /// else - no party record, no gold - then the whole save after it,
    /// whatever the landing.
    #[test]
    fn card_load_seeds_flags_before_the_entry_and_the_save_after_it() {
        let mut src = crate::world::World::default();
        src.system_flag_set(0x142);
        src.party.money = 4321;
        let mut save = src.save_full();
        save.party = legaia_save::Party::zeroed(4);

        for (enterable, running, want) in [
            (
                &["rikuroa"][..],
                None,
                ResumeLanding::SavedScene("rikuroa".into()),
            ),
            (
                &[][..],
                Some("town01"),
                ResumeLanding::CurrentScene("town01".into()),
            ),
        ] {
            let mut host = ProbeHost {
                world: crate::world::World::default(),
                running: running.map(String::from),
                enterable,
                seen_at_entry: Vec::new(),
                hydrated: false,
            };
            let landing = resume_card_load(&mut host, save.clone(), "rikuroa");
            assert_eq!(landing, want);
            assert_eq!(host.seen_at_entry.len(), 1);
            let (flag, roster, money) = host.seen_at_entry[0];
            assert!(flag, "the entry reads the saved story flags");
            assert_eq!(roster, 0, "no party record goes in ahead of the entry");
            assert_ne!(money, 4321, "nor the gold");
            assert_eq!(host.world.party.roster.members.len(), 4);
            assert_eq!(host.world.party.money, 4321);
            assert!(host.hydrated);
        }
    }

    #[test]
    fn saved_scene_wins_even_when_it_is_the_running_one() {
        // A Load of a save written in the open scene re-enters it: the entry
        // is what applies the save over fresh per-scene state.
        let mut calls = Vec::new();
        let landing = land_save("town03", Some("town03"), |s: &str| {
            calls.push(s.to_string());
            Ok::<(), String>(())
        });
        assert_eq!(landing, ResumeLanding::SavedScene("town03".into()));
        assert!(landing.entered_scene());
        assert_eq!(calls, ["town03"]);
    }

    #[test]
    fn unenterable_saved_scene_loads_over_the_running_scene() {
        let landing = land_save("nosuch", Some("map01"), enter_only(&["map01"]));
        assert_eq!(landing, ResumeLanding::CurrentScene("map01".into()));
        assert!(!landing.entered_scene());
        assert_eq!(landing.scene(), Some("map01"));
    }

    #[test]
    fn unenterable_saved_scene_with_nothing_running_enters_the_opening_town() {
        // Never a New Game: the prologue cutscene is not tried.
        let mut calls = Vec::new();
        let landing = land_save("nosuch", None, |s: &str| {
            calls.push(s.to_string());
            if s == OPENING_SCENE {
                Ok(())
            } else {
                Err("missing")
            }
        });
        assert_eq!(landing, ResumeLanding::OpeningTown);
        assert_eq!(calls, ["nosuch", OPENING_SCENE]);
        assert!(!calls.iter().any(|c| c == OPENING_CUTSCENE_SCENE));
    }

    #[test]
    fn save_without_a_scene_skips_straight_to_the_fallbacks() {
        let mut calls = 0;
        let landing = land_save("", Some("town01"), |_s: &str| {
            calls += 1;
            Ok::<(), String>(())
        });
        assert_eq!(landing, ResumeLanding::CurrentScene("town01".into()));
        assert_eq!(calls, 0);
        let landing = land_save("", None, enter_only(&[]));
        assert_eq!(landing, ResumeLanding::Nowhere);
        assert_eq!(landing.scene(), None);
    }

    #[test]
    fn empty_current_label_counts_as_nothing_running() {
        let landing = land_save("", Some(""), enter_only(&[OPENING_SCENE]));
        assert_eq!(landing, ResumeLanding::OpeningTown);
    }

    #[test]
    fn new_game_tries_the_cutscene_then_the_town() {
        assert_eq!(
            enter_new_game(enter_only(&[OPENING_CUTSCENE_SCENE, OPENING_SCENE])),
            Some(OPENING_CUTSCENE_SCENE)
        );
        assert_eq!(
            enter_new_game(enter_only(&[OPENING_SCENE])),
            Some(OPENING_SCENE)
        );
        assert_eq!(enter_new_game(enter_only(&[])), None);
    }

    #[test]
    fn landing_kinds_are_stable() {
        assert_eq!(ResumeLanding::SavedScene("a".into()).kind(), "saved");
        assert_eq!(ResumeLanding::CurrentScene("a".into()).kind(), "current");
        assert_eq!(ResumeLanding::OpeningTown.kind(), "opening");
        assert_eq!(ResumeLanding::Nowhere.kind(), "nowhere");
    }
}
