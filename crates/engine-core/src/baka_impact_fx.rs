//! Baka Fighter's **impact effect runtime**: the four `FUN_80021B04` parts
//! the impact pair (`FUN_801D4DF8`,
//! [`crate::baka_fighter_chrome::impact_effect_pair`]) spawns on a decided
//! exchange, stepped on the duel's own tick and turned into draws the duel
//! surface appends to its buffers.
//!
//! ## What the four templates are
//!
//! The spawn templates are move-VM records in the Baka Fighter overlay's
//! rodata (PROT 0976, base `0x801CE818`), `[i16 model_sel][u16 reserved]
//! [bytecode]`, the same record shape every `FUN_80021B04` caller hands it:
//!
//! - **A** (`0x801DB8FC` slot 0, `0x801DB960` slot 1): `model_sel = -1`, a
//!   transform node. Op `0x0C` builds the colour word `+0x74` (mode byte
//!   `0xC9`: ABE on, ABR 1 - additive), op `0x23` makes it a draw-kind-4 node
//!   on the `0x4000` **sprite arm** (`FUN_8002A5A4`) - one textured quad
//!   `0xA0` units square in the XY plane - and two `0x18`/`0x19` loops step
//!   its UV rect `0x20` texels per cell with op `0x24`, eight cells along one
//!   row and seven along the next, before op `0x08` halts it. A flip-book
//!   impact flash.
//! - **B** (`0x801DBBA4` slot 0, `0x801DBBD4` slot 1): `model_sel = 1` /
//!   `2`, a mesh node. `FUN_80021B04` adds `gp[+0x754]` (`0x8007BA6C`) to it,
//!   and the duel's init zeroes that word (`0x801CF1B0`) together with the
//!   scene-bank base `_DAT_8007B6F8` (`0x801CF1C0`), so the two meshes are
//!   the PROT 1203 stage pack's TMDs `1` and `2`. Op `0x0C` again sets mode
//!   byte `0xC9` with depth-cue level `+0x78 = 0x1000` (fully toward the
//!   colour word's black far colour - invisible under additive blending),
//!   op `0x0D` gives `+0x78` a falling rate, then a rising one: the mesh
//!   flashes in and fades out, drawn at the spawn's `-0x400` / `+0x400` yaw.
//!
//! ## Why this is not `world::ambient`
//!
//! The field's ambient part runtime
//! ([`crate::world::World::spawn_ambient_record_at`]) seats records out of
//! the **scene's** prescript stager table (`World::props.stagers`) and
//! flushes bytecode self-writes back into that bundle. These templates are
//! not in any stager table - `FUN_801D4DF8` passes their rodata addresses
//! straight to `FUN_80021B04` - and the standalone minigames page steps its
//! duel with no `World` at all. So the duel seats its parts here and drives
//! them through the same move-VM kernels the ambient tick uses
//! ([`legaia_engine_vm::move_vm::run_until_break`] /
//! [`legaia_engine_vm::move_vm::actor_tick`] /
//! [`legaia_engine_vm::move_vm::decrement_wait_timer`] /
//! [`legaia_engine_vm::move_vm::integrate_draw_channels`]), which keeps the
//! rules on [`crate::baka_fighter::BakaFight`] - the one tick every duel host
//! runs. None of the four templates spawns a child (no op `0x25`) or writes
//! its own bytecode, so the stager-bundle half of the ambient runtime has
//! nothing to do here.

use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};

/// Load base of the Baka Fighter overlay (PROT 0976).
const OVERLAY_BASE_VA: u32 = crate::baka_fighter::BAKA_OVERLAY_BASE_VA;

/// Words snapshotted per template. Retail hands the VM a pointer, so the
/// record has no length field; every template halts well inside this window
/// (the longest runs `0x32` words), and the disc-gated oracle asserts each
/// one reaches its `HALT` inside it.
pub const TEMPLATE_WINDOW_WORDS: usize = 0x40;

/// Per-tick opcode budget (a guard against a malformed stream only).
const PART_BUDGET: usize = 256;

/// Cap on live parts. A decided exchange spawns two, a draw four, and each
/// halts within a few dozen ticks; the cap only bounds a host that books
/// exchanges faster than any rule allows.
pub const MAX_IMPACT_PARTS: usize = 16;

/// The four spawn templates, in `[slot 0, slot 1]` order per pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactTemplate {
    /// Runtime VA of the record.
    pub va: u32,
    /// Record `+0` - the mesh selector (`-1` transform node, `>= 0` scene
    /// model).
    pub model_sel: i16,
    /// The record as u16 words from `+0`, [`TEMPLATE_WINDOW_WORDS`] long.
    pub words: Vec<u16>,
}

/// Both pairs of templates, read out of the as-loaded overlay image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactTemplates {
    /// [`crate::baka_fighter_chrome::IMPACT_TEMPLATE_A`] by slot.
    pub a: [ImpactTemplate; 2],
    /// [`crate::baka_fighter_chrome::IMPACT_TEMPLATE_B`] by slot.
    pub b: [ImpactTemplate; 2],
}

impl ImpactTemplates {
    /// Read the four records out of the as-loaded Baka Fighter overlay
    /// (`legaia_asset::static_overlay::as_loaded` of PROT 0976). `None` when
    /// the image is too short to hold them.
    pub fn from_overlay(overlay_0976: &[u8]) -> Option<Self> {
        let read = |va: u32| -> Option<ImpactTemplate> {
            let off = va.checked_sub(OVERLAY_BASE_VA)? as usize;
            let bytes = overlay_0976.get(off..off + TEMPLATE_WINDOW_WORDS * 2)?;
            let words: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            Some(ImpactTemplate {
                va,
                model_sel: words[0] as i16,
                words,
            })
        };
        use crate::baka_fighter_chrome::{IMPACT_TEMPLATE_A, IMPACT_TEMPLATE_B};
        Some(Self {
            a: [read(IMPACT_TEMPLATE_A[0])?, read(IMPACT_TEMPLATE_A[1])?],
            b: [read(IMPACT_TEMPLATE_B[0])?, read(IMPACT_TEMPLATE_B[1])?],
        })
    }

    fn by_va(&self, va: u32) -> Option<&ImpactTemplate> {
        self.a.iter().chain(self.b.iter()).find(|t| t.va == va)
    }
}

/// The move-VM host a duel part runs under. Every hook keeps its default:
/// the templates reach none of them (no spawn, no extension op, no
/// bytecode write), and a hook a malformed record reached would have no
/// duel-side meaning.
struct PartHost;

impl MoveHost for PartHost {}

/// One live impact part: the actor record the VM mutates, and the template
/// it runs.
#[derive(Debug, Clone)]
pub struct ImpactPart {
    /// The template's VA.
    pub template: u32,
    /// Record `+0`.
    pub model_sel: i16,
    words: std::sync::Arc<[u16]>,
    /// The part's actor fields.
    pub state: ActorState,
    /// Set on the tick its VM halts; the part draws on that tick and is
    /// freed on the next (retail's actor walk skips a halted actor's tick
    /// function from then on - `FUN_8002519C`).
    pub halted: bool,
}

impl ImpactPart {
    /// Seat `template` at `pos` / `rot` with render scale `scale` and run
    /// its first VM slice immediately.
    ///
    /// PORT: FUN_80021B04 - the stager's seat for the two record shapes the
    /// duel uses (`0x80021B94..0x80021DD0`): world position into `+0x14`, a
    /// transform node (`model_sel < 0`) takes draw kind `+0x56 = 0` and
    /// mode `+0x5A = 0` with `+0x10 |= 2`, a mesh node takes `+0x5A = 1` and
    /// `+0x10 |= 0x08000000` (the model binding, `+0x64 = model_sel +
    /// gp[+0x754]`, is the draw's business - [`ImpactDraw::Mesh::model`]),
    /// then PC `+0x70 = 2`, `+0xC4 = +0xC6 = 0`, `+0x10 |=
    /// 0x4000` (and `&= !2` for a mesh), `+0x3C..+0x40 = 0`, the rotation
    /// banks `+0x24..+0x28` and scale `+0x72`, and the first VM call
    /// (`jal 0x80023070` at `0x80021DC0`, not gated on the wait timer).
    /// The `0x4000` / `0x4001` render-node arms are not reached by these
    /// templates and are not modelled.
    pub fn spawn(template: &ImpactTemplate, pos: [i16; 3], rot: [i16; 3], scale: u16) -> Self {
        let mut st = ActorState::new();
        st.world_x = pos[0];
        st.world_y = pos[1];
        st.world_z = pos[2];
        if template.model_sel < 0 {
            st.move_substate = 0;
            st.move_submode = 0;
            st.flags |= 0x2;
        } else {
            st.move_submode = 1;
            st.flags |= 0x0800_0000;
        }
        st.pc = 2;
        st.set_actor_u16(0xC4, 0);
        st.set_actor_u16(0xC6, 0);
        st.flags |= 0x4000;
        if template.model_sel >= 0 {
            st.flags &= !0x2;
        }
        st.anim_3c = 0;
        st.anim_3e = 0;
        st.anim_40 = 0;
        st.render_24 = rot[0];
        st.render_26 = rot[1];
        st.render_28 = rot[2];
        st.field_72 = scale;
        let words: std::sync::Arc<[u16]> = template.words.clone().into();
        let _ = move_vm::run_until_break(&mut PartHost, &mut st, &words, PART_BUDGET);
        st.world_y_mirror = st.world_y;
        let halted = st.flags & 0x8 != 0;
        ImpactPart {
            template: template.va,
            model_sel: template.model_sel,
            words,
            state: st,
            halted,
        }
    }

    /// One part tick at `delta = DAT_1F800393 * DAT_1F80037D` (the duel's
    /// frame step times its rate divisor - the special's slow motion slows
    /// the effect too).
    ///
    /// PORT: FUN_80021DF4 (`0x80021E2C..0x80021E4C` the wait drain,
    /// `0x80021E78..0x80021FA0` the mode-2 channel block, `0x800228B8..
    /// 0x80022B90` the motion block every mode but `3` / `5` runs,
    /// `0x80022B94..0x80022BBC` the VM gate, `0x80022BC0..0x80022C1C` the
    /// `+0x78` / `+0x72` clamps). What is left out: the `+0x22` spin
    /// (`+0xD0` rate - op `0x3F`, which no template issues), the heading
    /// term of the position integrator (`+0x96` / `+0x98` through the sin
    /// tables at `0x8007B81C` / `0x8007B7F8` - no template writes `+0x98`,
    /// so it is zero) and the `+0x86 & 0x2000` field cull call
    /// (`FUN_801D79E8`, a field-overlay routine; no template sets the bit).
    pub fn tick(&mut self, delta: u16) {
        if self.halted {
            return;
        }
        let st = &mut self.state;
        move_vm::decrement_wait_timer(st, delta);
        move_vm::integrate_draw_channels(st, delta);
        if st.move_submode != 3 && st.move_submode != 5 {
            motion_block(st, delta);
        }
        let outcome = move_vm::actor_tick(&mut PartHost, st, &self.words, PART_BUDGET);
        if matches!(
            outcome,
            ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. }
        ) {
            self.halted = true;
        }
        if st.field_78 > 0x3E80 {
            st.field_78 = 0;
        }
        if st.field_78 > 0x1000 {
            st.field_78 = 0x1000;
        }
        if st.field_72 > 0x3E80 {
            st.field_72 = 0;
        }
        if st.field_72 > 0x3A98 {
            st.field_72 = 0x3A98;
        }
    }

    /// The part's draw for this tick, or `None` for a part that draws
    /// nothing (a transform node that never became a sprite).
    pub fn draw(&self) -> Option<ImpactDraw> {
        let st = &self.state;
        let colour = ColourWord::of(st);
        if self.model_sel >= 0 {
            return Some(ImpactDraw::Mesh {
                model: self.model_sel as usize,
                pos: [st.world_x, st.world_y, st.world_z],
                rot: [st.render_24, st.render_26, st.render_28],
                scale: st.field_72,
                colour,
            });
        }
        if st.move_substate != 4 || st.field_9e & 0x4000 == 0 {
            return None;
        }
        let quad = sprite_arm_quad(st.field_9e, st)?;
        Some(ImpactDraw::Sprite {
            quad,
            pos: [st.world_x, st.world_y, st.world_z],
            rot: [st.render_24, st.render_26, st.render_28],
            scale: st.field_72,
            colour,
        })
    }
}

/// The motion block of the part tick: the rotation banks step by their
/// rates, the position by its velocity, and the render scale, `+0x7A` and
/// depth-cue level by theirs - each `(rate * delta) >> 6`.
///
/// `0x800228B8..0x80022B90`: `+0x24/+0x26/+0x28 += +0x80/+0x82/+0x84`,
/// `+0x14 += (+0x3C << 12) * delta >> 18` (heading term omitted, see
/// [`ImpactPart::tick`]), `+0x16` and `+0x2A += +0x3E`,
/// `+0x18 += (+0x40 << 12) * delta >> 18`, `+0x72 += +0x92`,
/// `+0x7A += +0x94`, `+0x78 += +0x90`, then a negative `+0x7A` is zeroed.
fn motion_block(st: &mut ActorState, delta: u16) {
    let d = i32::from(delta);
    let step = |rate: i16| -> i16 { ((i32::from(rate) * d) >> 6) as i16 };
    st.render_24 = st.render_24.wrapping_add(step(st.anim_80));
    st.render_26 = st.render_26.wrapping_add(step(st.anim_82));
    st.render_28 = st.render_28.wrapping_add(step(st.anim_84));
    st.world_x = st
        .world_x
        .wrapping_add((((i32::from(st.anim_3c) << 12) * d) >> 18) as i16);
    let dy = step(st.anim_3e);
    st.world_y = st.world_y.wrapping_add(dy);
    st.world_y_mirror = st.world_y_mirror.wrapping_add(dy);
    st.world_z = st
        .world_z
        .wrapping_add((((i32::from(st.anim_40) << 12) * d) >> 18) as i16);
    st.field_72 = st.field_72.wrapping_add(step(st.tween_src_y) as u16);
    st.field_7a = st.field_7a.wrapping_add(step(st.tween_src_z) as u16);
    st.field_78 = st.field_78.wrapping_add(step(st.tween_src_x) as u16);
    if (st.field_7a as i16) < 0 {
        st.field_7a = 0;
    }
}

/// The colour word `+0x74` and depth-cue level `+0x78` the draw hands the
/// prim dispatcher `FUN_80043390`, decoded the way that routine decodes
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourWord {
    /// Bit 31 - the GP0 ABE bit the dispatcher ORs into every packet
    /// (`srl s6,a1,0x1f; sll s6,s6,0x19` at `0x80043510`).
    pub semi: bool,
    /// `(a1 >> 24) & 3` - the ABR mode ORed into the packets' tpage bits
    /// (`0x80043504..0x8004350C`).
    pub abr: u8,
    /// The low 24 bits - the GTE far colour the depth cue blends toward
    /// (`ctc2` into `RFC/GFC/BFC` at `0x800434C8..0x800434D0`).
    pub far: [u8; 3],
    /// The depth-cue level, `IR0` for every fog-bank prim (`lwc2 t0,
    /// -0x2DC(t2)` in the bank handlers) - `+0x78`, with bit 0 forced when
    /// the mode byte is non-zero (`ori a2,a2,1` at `0x800433CC`).
    pub ir0: u16,
}

impl ColourWord {
    /// Decode the part's `+0x74` word and `+0x78` level.
    pub fn of(st: &ActorState) -> Self {
        let w = st.field_74;
        let mode = (w >> 24) as u8;
        let mut ir0 = st.field_78;
        if mode != 0 {
            ir0 |= 1;
        }
        ColourWord {
            semi: w >> 31 != 0,
            abr: mode & 3,
            far: [w as u8, (w >> 8) as u8, (w >> 16) as u8],
            ir0: ir0.min(0x1000),
        }
    }

    /// A packet colour after the depth cue: `c + (far - c) * IR0 >> 12`
    /// per channel (the GTE `DPCS` the fog banks run).
    pub fn cue(&self, c: [u8; 3]) -> [u8; 3] {
        let ir0 = i32::from(self.ir0);
        std::array::from_fn(|k| {
            let (c, f) = (i32::from(c[k]), i32::from(self.far[k]));
            (c + (((f - c) * ir0) >> 12)).clamp(0, 255) as u8
        })
    }
}

/// One quad the `0x4000` sprite arm builds, in the part's model space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteArmQuad {
    /// The four vertices, packet order `0..3` (the index words
    /// `0x00080000` / `0x00180010`).
    pub verts: [[i16; 3]; 4],
    /// Each vertex's texel.
    pub uvs: [[u8; 2]; 4],
    /// The CLUT word (hi half of the first UV word).
    pub clut: u16,
    /// The tpage word (hi half of the second UV word).
    pub tpage: u16,
    /// The packet colour (`+0xA0`'s low 24 bits).
    pub rgb: [u8; 3],
    /// `GT4` (`0x3C`) when `(mode >> 3) & 7 == 0`, else `FT4` (`0x2C`). The
    /// four colours are one word either way.
    pub gouraud: bool,
}

/// PORT: FUN_8002A5A4 - the render dispatcher's `0x4000` **sprite arm**:
/// one textured quad out of a draw-kind-4 node's `+0x9C` block.
///
/// `mode` is `+0x9E`; its low two bits pick the plane - `0` puts the quad in
/// XY, `1` in XZ, `2` in ZY - with the vertices at `(∓w/2, ∓h/2)` in packet
/// order (`w = +0xB4`, `h = +0xB6`, each halved toward zero -
/// `srl 0x1f; addu; sra 1`). Mode `3` reaches the vertex stores through
/// three pointers no arm initialised (`0x8002A628` falls to `0x8002A67C`
/// with `a2` still the call's count argument), so it returns `None` rather
/// than inventing a plane. The UV words are built with an **unmasked**
/// `addu` chain - `(clut << 16) + (v0 << 8) + u0`, `(tpage << 16) + (v0 << 8)
/// + u1`, `((v1 << 8) + u0) + (((v1 << 8) + u1) << 16)` - so a halfword past
/// `0xFF` carries into the byte above it, and that is what the GPU reads;
/// the port packs and unpacks the same words. `u0 = +0xA8`, `v0 = +0xAA`,
/// `u1 = +0xAC`, `v1 = +0xAE`, `tpage = +0xB0`, `clut = +0xB2`.
pub fn sprite_arm_quad(mode: u16, st: &ActorState) -> Option<SpriteArmQuad> {
    let half =
        |v: i16| -> i16 { ((i32::from(v) + ((i32::from(v) as u32 >> 31) as i32)) >> 1) as i16 };
    let hw = half(st.actor_u16(0xB4) as i16);
    let hh = half(st.actor_u16(0xB6) as i16);
    let corners = [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh)];
    let verts = match mode & 3 {
        0 => corners.map(|(a, b)| [a, b, 0]),
        1 => corners.map(|(a, b)| [a, 0, b]),
        2 => corners.map(|(a, b)| [0, b, a]),
        _ => return None,
    };
    let u0 = u32::from(st.actor_u16(0xA8));
    let v0 = u32::from(st.actor_u16(0xAA));
    let u1 = u32::from(st.actor_u16(0xAC));
    let v1 = u32::from(st.actor_u16(0xAE));
    let tpage = u32::from(st.actor_u16(0xB0));
    let clut = u32::from(st.actor_u16(0xB2));
    let w1 = (clut << 16).wrapping_add(v0 << 8).wrapping_add(u0);
    let w2 = (tpage << 16).wrapping_add(v0 << 8).wrapping_add(u1);
    let w3 = ((v1 << 8).wrapping_add(u0)).wrapping_add(((v1 << 8).wrapping_add(u1)) << 16);
    let uv = |w: u32| [w as u8, (w >> 8) as u8];
    let colour = st.actor_u32(0xA0);
    Some(SpriteArmQuad {
        verts,
        uvs: [uv(w1), uv(w2), uv(w3), uv(w3 >> 16)],
        clut: (w1 >> 16) as u16,
        tpage: (w2 >> 16) as u16,
        rgb: [colour as u8, (colour >> 8) as u8, (colour >> 16) as u8],
        gouraud: (mode >> 3) & 7 == 0,
    })
}

/// What one live part draws this tick. Positions are raw retail world
/// coordinates (Y down), the duel surface's frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactDraw {
    /// A sprite-arm quad (template A).
    Sprite {
        quad: SpriteArmQuad,
        pos: [i16; 3],
        rot: [i16; 3],
        scale: u16,
        colour: ColourWord,
    },
    /// A scene model (template B): `model` is the global TMD index, which on
    /// the duel is the PROT 1203 stage pack's index.
    Mesh {
        model: usize,
        pos: [i16; 3],
        rot: [i16; 3],
        scale: u16,
        colour: ColourWord,
    },
}

/// The duel's live impact parts.
#[derive(Debug, Clone, Default)]
pub struct ImpactFx {
    templates: Option<std::sync::Arc<ImpactTemplates>>,
    parts: Vec<ImpactPart>,
}

impl ImpactFx {
    /// Install the templates; without them [`Self::spawn_pair`] spawns
    /// nothing.
    pub fn with_templates(templates: ImpactTemplates) -> Self {
        Self {
            templates: Some(std::sync::Arc::new(templates)),
            parts: Vec::new(),
        }
    }

    /// `true` once templates are installed.
    pub fn has_templates(&self) -> bool {
        self.templates.is_some()
    }

    /// Seat the impact pair's two spawns
    /// ([`crate::baka_fighter_chrome::impact_effect_pair`]). Returns how many
    /// parts were seated.
    pub fn spawn_pair(&mut self, spawns: &[crate::baka_fighter_chrome::ImpactSpawn; 2]) -> usize {
        let Some(t) = self.templates.clone() else {
            return 0;
        };
        let mut n = 0;
        for s in spawns {
            if self.parts.len() >= MAX_IMPACT_PARTS {
                break;
            }
            let Some(tpl) = t.by_va(s.template) else {
                continue;
            };
            let pos = [s.pos.0, s.pos.1, s.pos.2];
            let rot = [s.rot.0, s.rot.1, s.rot.2];
            self.parts
                .push(ImpactPart::spawn(tpl, pos, rot, s.scale as u16));
            n += 1;
        }
        n
    }

    /// One duel tick: free the parts that halted on an earlier tick, then
    /// step the rest at `delta`.
    pub fn tick(&mut self, delta: u16) {
        self.parts.retain(|p| !p.halted);
        for p in &mut self.parts {
            p.tick(delta);
        }
    }

    /// Drop every part (a round setup / rung install).
    pub fn clear(&mut self) {
        self.parts.clear();
    }

    /// The live parts.
    pub fn parts(&self) -> &[ImpactPart] {
        &self.parts
    }

    /// Every live part's draw.
    pub fn draws(&self) -> Vec<ImpactDraw> {
        self.parts.iter().filter_map(|p| p.draw()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic sprite template in the shipped shape: colour word, a
    /// sprite node, one two-cell loop, halt.
    fn sprite_template() -> ImpactTemplate {
        let mut w: Vec<u16> = vec![
            0xFFFF, 0, // model_sel -1, reserved
            0x0C, 0xFF89, 0, 0, 0, 0, // colour word, mode 0xC9
            0x23, 0, 0x80, 0x80, 0x80, 0xA0, 0xA0, 0x15, 0x7DC0, 0, 0, 0x1F, 0x1F, 0x18,
            1, // loop twice
            0x09, 1, // wait
            0x24, 0x20, 0,    // step the cell
            0x19, // loop back
            0x08, // halt
        ];
        w.resize(TEMPLATE_WINDOW_WORDS, 0);
        ImpactTemplate {
            va: crate::baka_fighter_chrome::IMPACT_TEMPLATE_A[0],
            model_sel: -1,
            words: w,
        }
    }

    #[test]
    fn a_sprite_template_draws_a_quad_and_steps_its_cell() {
        let t = sprite_template();
        let mut p = ImpactPart::spawn(&t, [10, -20, 30], [0; 3], 0x1000);
        let Some(ImpactDraw::Sprite {
            quad, pos, colour, ..
        }) = p.draw()
        else {
            panic!("sprite draw");
        };
        assert_eq!(pos, [10, -20, 30]);
        assert_eq!(quad.verts[0], [-0x50, -0x50, 0]);
        assert_eq!(quad.verts[3], [0x50, 0x50, 0]);
        assert_eq!(quad.uvs, [[0, 0], [0x1F, 0], [0, 0x1F], [0x1F, 0x1F]]);
        assert_eq!((quad.tpage, quad.clut), (0x15, 0x7DC0));
        assert_eq!(quad.rgb, [0x80, 0x80, 0x80]);
        assert!(colour.semi);
        assert_eq!(colour.abr, 1);
        let first_u = quad.uvs[0][0];
        let mut stepped = false;
        for _ in 0..64 {
            p.tick(8);
            if let Some(ImpactDraw::Sprite { quad, .. }) = p.draw()
                && quad.uvs[0][0] != first_u
            {
                stepped = true;
            }
            if p.halted {
                break;
            }
        }
        assert!(stepped, "the loop moves the cell");
        assert!(p.halted, "the template halts");
    }

    #[test]
    fn the_uv_words_carry_like_the_addu_chain() {
        let mut st = ActorState::new();
        st.set_actor_u16(0xB4, 0x20);
        st.set_actor_u16(0xB6, 0x20);
        st.set_actor_u16(0xA8, 0x100); // u0 past a byte: carries into v
        st.set_actor_u16(0xAA, 0x20);
        st.set_actor_u16(0xAC, 0x11F);
        st.set_actor_u16(0xAE, 0x3F);
        st.set_actor_u16(0xB0, 0x15);
        st.set_actor_u16(0xB2, 0x7DC0);
        let q = sprite_arm_quad(0, &st).unwrap();
        assert_eq!(q.uvs[0], [0x00, 0x21]);
        assert_eq!(q.uvs[1], [0x1F, 0x21]);
        assert_eq!(q.uvs[2], [0x00, 0x40]);
        assert_eq!(q.uvs[3], [0x1F, 0x40]);
        assert!(sprite_arm_quad(3, &st).is_none(), "mode 3 has no plane");
        assert_eq!(sprite_arm_quad(1, &st).unwrap().verts[0], [-0x10, 0, -0x10]);
        assert_eq!(sprite_arm_quad(2, &st).unwrap().verts[1], [0, -0x10, 0x10]);
    }

    #[test]
    fn a_mesh_template_fades_in_then_out_through_the_depth_cue() {
        let mut w: Vec<u16> = vec![
            1, 0, // model 1
            0x0C, 0xFF89, 0, 0, 0, 0x1000, // colour word, cue fully to black
            0x0D, 0xFC00, // falling cue rate
            0x09, 4, // wait
            0x0D, 0x0199, // rising cue rate
            0x09, 0x0F, // wait
            0x08,
        ];
        w.resize(TEMPLATE_WINDOW_WORDS, 0);
        let t = ImpactTemplate {
            va: crate::baka_fighter_chrome::IMPACT_TEMPLATE_B[0],
            model_sel: 1,
            words: w,
        };
        let mut p = ImpactPart::spawn(&t, [0; 3], [0, -0x400, 0], 0x1000);
        let cue = |p: &ImpactPart| match p.draw() {
            Some(ImpactDraw::Mesh { colour, model, .. }) => {
                assert_eq!(model, 1);
                colour.ir0
            }
            _ => panic!("mesh draw"),
        };
        assert_eq!(cue(&p), 0x1000, "spawns invisible");
        let mut lowest = 0x1000;
        let mut rose_after = false;
        for _ in 0..200 {
            p.tick(8);
            if p.halted {
                break;
            }
            let c = cue(&p);
            if c < lowest {
                lowest = c;
            } else if c > lowest {
                rose_after = true;
            }
        }
        assert!(lowest <= 1, "flashes to full colour");
        assert!(rose_after, "fades back");
        assert!(p.halted);
    }

    #[test]
    fn the_depth_cue_lerps_toward_the_far_colour() {
        let c = ColourWord {
            semi: true,
            abr: 1,
            far: [0, 0, 0],
            ir0: 0x800,
        };
        assert_eq!(c.cue([0x80, 0x40, 0xFF]), [0x40, 0x20, 0x7F]);
    }
}
