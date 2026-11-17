//! The world-overview page's **posed landmarks**: the overworld objects whose
//! object bind names a clip, posed at the frame their spawn prologue picks.
//!
//! A kingdom map's placed landmarks are actors (`FUN_8003A55C`), and a bound
//! one whose bind header carries an anim byte draws through `FUN_8001B964`
//! (draw kind `1`) at its clip frame. Its prologue runs at spawn and its flag
//! tests pick the arm: Conkram's shell lid (`map03` record 458, pack mesh 16,
//! clip 4) runs `74 C8 07 00` over a `4C 35` arm (frame 0, the lid standing
//! open behind the town) and a `4C 36` arm (its last frame, shut over the
//! town). The page's walk stamps resolve straight from the `.MAP`, unbound
//! and unposed, so the lid lay flat in front of the town.
//!
//! This is the same bank the play hosts build on a world-map entry
//! ([`PropAnimBank::build_with_flags`], `SceneHost::install_field_props`),
//! run against a page-chosen story flag set instead of a live world, and the
//! same pose kernel ([`posed_prop_offsets`]) the play hosts feed their mesh
//! builders.
// REF: FUN_8003A55C, FUN_8001B964

use std::collections::{BTreeMap, BTreeSet, HashMap};

use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_engine_scene::field_env::{
    ObjectBind, PropAnimBank, PropPoseKey, SpawnFlags, posed_prop_offsets,
};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::FlagKind;
use legaia_scene_formats::field_objects::Placement;

/// One kingdom's posable landmarks: the walk map's placed objects, their
/// binds, the MAN their bind records live in and the scene ANM bundle their
/// clips index.
pub struct KingdomLandmarks {
    placements: Vec<Placement>,
    binds: HashMap<(u8, u8), ObjectBind>,
    man: Vec<u8>,
    anm: Option<PlayerAnmBundle>,
}

/// The landmark poses under one story flag set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LandmarkPoses {
    /// `(anim_id, pose key)` per bound landmark anchor whose bind names a
    /// clip.
    pub by_anchor: BTreeMap<(u8, u8), (u8, PropPoseKey)>,
    /// The story flags the posed landmarks' prologues test - the flags that
    /// can change what the overview draws.
    pub tested_flags: BTreeSet<u16>,
}

impl KingdomLandmarks {
    /// Load `scene_name`'s (`map01` / `map02` / `map03`) walk landmarks.
    /// `None` when the scene, its walk map, its binds or its MAN do not
    /// resolve.
    pub fn load(index: &ProtIndex, scene_name: &str) -> Option<Self> {
        let scene = Scene::load(index, scene_name).ok()?;
        let placements = scene.walk_object_placements(index).ok()??;
        let binds = scene.field_object_binds(index).ok()??;
        let man = scene.field_man_payload(index).ok()??;
        let anm = legaia_engine_scene::npc_catalog::scene_anm_bundle(&scene);
        Some(Self {
            placements,
            binds,
            man,
            anm,
        })
    }

    /// The bind's clip id of the landmark anchored at `anchor`, `0` when it
    /// has none.
    pub fn anim_id(&self, anchor: (u8, u8)) -> u8 {
        self.binds.get(&anchor).map_or(0, |b| b.anim_id)
    }

    /// Run every bound landmark's spawn prologue against `flags` (a flag in
    /// the set reads as raised; the prologues' own writes land in a copy, in
    /// placement order, as they do on the live world) and report each posed
    /// landmark's pose.
    pub fn poses(&self, flags: &BTreeSet<u16>) -> LandmarkPoses {
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&self.man) else {
            return LandmarkPoses::default();
        };
        let bank = self.bank(&man_file, &self.placements, flags, None);
        let mut by_anchor = BTreeMap::new();
        let mut tested_flags = BTreeSet::new();
        for (&anchor, state) in &bank.props {
            let anim_id = self.anim_id(anchor);
            if anim_id == 0 {
                continue;
            }
            by_anchor.insert(anchor, (anim_id, state.anim.pose_key()));
            // The flags this landmark's own prologue tests: its pass alone,
            // recorded.
            let own: Vec<Placement> = self
                .placements
                .iter()
                .filter(|p| (p.anchor_col, p.anchor_row) == anchor)
                .take(1)
                .copied()
                .collect();
            self.bank(&man_file, &own, flags, Some(&mut tested_flags));
        }
        LandmarkPoses {
            by_anchor,
            tested_flags,
        }
    }

    /// [`PropAnimBank::build_with_flags`] over `placements`, each prologue's
    /// flag tests reading `flags` (its writes land in a copy, in placement
    /// order, as on the live world); `tested` collects the flags tested.
    fn bank(
        &self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        placements: &[Placement],
        flags: &BTreeSet<u16>,
        mut tested: Option<&mut BTreeSet<u16>>,
    ) -> PropAnimBank {
        let clip = |anim: u8| -> Option<(u16, bool, u8)> {
            let r = self
                .anm
                .as_ref()?
                .record(anim.checked_sub(1)? as usize)
                .ok()?;
            Some((r.frame_count, (r.a >> 8) & 1 != 0, (r.flag & 0xFF) as u8))
        };
        let mut live = flags.clone();
        let mut on_flag = |idx: u16, kind: FlagKind| -> bool {
            match kind {
                FlagKind::Test => {
                    if let Some(t) = tested.as_deref_mut() {
                        t.insert(idx);
                    }
                    live.contains(&idx)
                }
                FlagKind::Set => {
                    live.insert(idx);
                    false
                }
                FlagKind::Clear => {
                    live.remove(&idx);
                    false
                }
            }
        };
        let spawn_flags: &mut SpawnFlags<'_> = &mut on_flag;
        PropAnimBank::build_with_flags(
            placements,
            &self.binds,
            man_file,
            &self.man,
            clip,
            Some(spawn_flags),
        )
    }

    /// Per-object `(translation, rotation)` offsets of clip `anim_id` at
    /// `key` for a mesh of `object_count` objects, `None` when the clip does
    /// not resolve or its bone count differs (retail's count-equality
    /// contract).
    pub fn offsets(
        &self,
        anim_id: u8,
        key: PropPoseKey,
        object_count: usize,
    ) -> Option<Vec<([i16; 3], [i16; 3])>> {
        posed_prop_offsets(self.anm.as_ref()?, anim_id, key, object_count)
    }
}
