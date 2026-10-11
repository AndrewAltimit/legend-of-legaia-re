//! Seru-magic **summon scene-graph driver** (engine stand-in render - see the
//! reconciliation note below; this is **not** the faithful player-summon path).
//!
//! The per-summon stager overlay (extraction PROT 903..=913 = raw TOC
//! `0x389..=0x393`; Gimard *Burning Attack* = 903)
//! carries real move-VM scene-graph part records, recovered by
//! [`legaia_overlay_images::summon_overlay`]. This module drives them: it seeds one
//! move-VM [`ActorState`] per part from its record and ticks every part through
//! the **already-ported move VM** each frame, exactly as the retail spawn helper
//! `FUN_80021B04` stages a part (`actor[+0x48]` = record move-buffer base,
//! `actor[+0x70] = 2` PC → bytecode at `record+4`) and the per-frame actor tick
//! `FUN_80021DF4` runs `FUN_80023070` on it.
//!
//! ## Reconciliation: this is not how retail renders the player summon
//!
//! A live PCSX-Redux trace of a player Gimard *Burning Attack* cast (scenarios
//! `gimard_summon_start` / `_visible` / `gimard_burning_attack`) shows the
//! rendered summon is an **ordinary battle actor**, not this move-VM scene-graph:
//! across all three phases `FUN_801F7088` fired **0×**, the move VM
//! `FUN_80023070` fired only **2-3×** (trace noise, not a per-part driver), and
//! the **battle per-actor draw `FUN_80048A08` ran once per live actor per
//! rendered frame** → the per-object rigid-TRS keyframe decoder `FUN_8004998C`
//! → cluster-A `FUN_80043390`. (The earlier "35-64×/frame" magnitude for the
//! same address does not reproduce: a 400-vsync re-measurement on the
//! catalogued `gimard_burning_attack` state counts 213 calls and never more
//! than 2 per rendered frame - `docs/subsystems/effect-vm.md`.) So the player
//! summon is posed exactly like an enemy monster
//! body (per-object rigid TRS keyframes), and the **faithful render path is the
//! battle TRS-keyframe draw already ported in
//! [`legaia_engine_vm::anim_vm`]** (`FUN_80048A08` / `FUN_8004998C`), *not* this
//! [`SummonScene`].
//!
//! The faithful **player** summon render is already wired: a cast routes through
//! `crate::world::World::request_summon_spawn` →
//! [`crate::world::World::take_pending_summon_spawn`], and the host spawns the
//! summon's namesake `battle_data` creature ([`summon_creature_id`]) as an
//! ordinary battle actor - mesh + texture via
//! `legaia_actor_formats::monster_archive::battle_render_mesh`, animation via
//! [`legaia_actor_formats::monster_archive::idle_animation`] →
//! [`crate::battle_anim::MonsterAnimPlayer`] → the rigid TRS-keyframe draw
//! (`FUN_80048A08` / `FUN_8004998C`, ported in `legaia_engine_vm::anim_vm`). That
//! is the production summon visual.
//!
//! [`SummonScene`] is therefore *not* the production render - it is kept because
//! (1) it is the validated parser/driver for the genuinely-on-disc move-VM stager
//! records (disc-gated `summon_scene_real` - every Gimard part runs the move VM
//! without an unimplemented opcode); (2) it backs the non-battle debug spawn
//! (`play-window` `G` outside battle) that exercises the move-VM driver; and
//! (3) the **enemy** Gimard *Fire Tail* boss move is untraced and may still use
//! the overlay/move-VM path, so this remains its candidate model. The live trace
//! that resolved the player path covers the **player** "Burning Attack" only. See
//! the open-rev-eng-threads "Seru-magic summon visual" row for the full
//! reconciliation.
//!
//! ## What is faithful vs. interpreted (within this stand-in model)
//!
//! - **Faithful:** the per-part animation *computation*. Each part's move-VM
//!   program runs through [`legaia_engine_vm::move_vm`] verbatim - the same
//!   opcode handlers, wait-timer gate, and tween/anim-bank state the retail move
//!   VM produces. (Validated: every Gimard *Tail Fire* part record runs without
//!   hitting an unimplemented opcode.)
//! - **Interpreted (translation glide):** each frame the world position
//!   (`+0x14/16/18`) glides toward the move-VM anim-bank target
//!   (`anim_3c/3e/40`, op `0x00`, `v << 3`) over the time ratio
//!   `+0x9C / +0x9E`, latching exactly on completion. The lerp *shape* reuses
//!   `FUN_801F811C` / `FUN_801DE4C8` mode 1 - but the retail `FUN_801F811C` is
//!   **not a summon-part position update**: a full static decode of PROT 0900
//!   at the correct link base resolved it as the per-frame handler of the 2D
//!   **screen-mask (iris) widget**, whose four tweened channels are screen-rect
//!   edges and whose "4 render quads" are the black border bands
//!   (see [`crate::screen_fx`], the faithful port). Two deviations of this
//!   glide from the retail tween math, kept deliberately for the stand-in:
//!   the engine applies it to move-VM part positions (retail does not), and it
//!   re-lerps **iteratively from the updated current value** each frame,
//!   whereas the retail widget tween re-interpolates from a start value that
//!   stays fixed until the latch.
//! - **Interpreted (rotation):** the part's *orientation*. Within this stand-in
//!   model [`SummonScene::part_draws`] derives Euler angles from the move-VM
//!   rotation banks. This is the engine's interpretation of the scene-graph
//!   model, **not** a faithful port of retail's player-summon orientation -
//!   which the live trace resolved as the per-object TRS keyframes that
//!   `FUN_8004998C` decodes (the battle-actor path above), not a move-VM /
//!   `FUN_801F7088` rotation. (PROT 0900's `RotMatrixX/Y/Z` + camera-angle
//!   `FUN_801F7088` build is the **world-map top-view tile renderer** aliasing
//!   the same `0x801Fxxxx` band, not the battle-summon code.)

use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};
use legaia_overlay_images::summon_overlay::{SummonOverlay, SummonPart};

mod part_draw;
mod scene;
mod spell_catalog;
mod translation;

pub use part_draw::*;
pub use scene::*;
pub use spell_catalog::*;
pub use translation::*;

#[cfg(test)]
mod tests;
