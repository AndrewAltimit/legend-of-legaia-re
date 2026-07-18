//! Screen-fade primitive state - clean-room port of the retail fade-state
//! loader (`FUN_80020B00`, `see ghidra/scripts/funcs/80020b00.txt`).
//!
//! Retail stages full-screen fades as pool actors: `FUN_80024E80` allocates an
//! actor and calls the loader with a 13-`i16` template describing the ramp.
//! The loader converts the template into a 10.6 fixed-point state:
//!
//! ```text
//! state[0..2]  = start RGB << 6          ; current colour (10.6 fixed)
//! state[4..6]  = end RGB << 6
//! state[8..10] = ((end - start) * 0x40) / duration   ; per-frame delta
//! state[0x10]  = duration (frames)
//! ```
//!
//! so the displayed colour each frame is `current >> 6`, advancing linearly
//! and landing exactly on `end` after `duration` frames. The battle-action SM
//! stages the summon backdrop fade (state `0x33`) and the successful-escape
//! white-out (state `0x66`, template at `DAT_801C9070`) through this.

/// The 13-`i16` fade template `FUN_80020B00` consumes (`param_2` field
/// indices in brackets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeTemplate {
    /// `[0]` - fade kind/id, copied verbatim onto the state.
    pub kind: i16,
    /// `[1]` - ramp duration in frames (the per-frame delta divisor).
    pub duration: i16,
    /// `[3..=5]` - start RGB.
    pub start_rgb: [i16; 3],
    /// `[7..=9]` - end RGB.
    pub end_rgb: [i16; 3],
    /// `[10]` / `[11]` / `[12]` - mode words copied verbatim onto the state
    /// (consumed by the pool actor's draw handler; semantics not yet pinned).
    pub mode: [i16; 3],
}

/// The successful-escape white-out template the battle-action SM writes at
/// `DAT_801C9070` before spawning the fade (state `0x66`): kind `2`, a
/// `0x40`-frame ramp from black `(0,0,0)` to white `(0xFF,0xFF,0xFF)`,
/// mode words `(0, -1, 0)`.
///
/// REF: FUN_801E295C (case 0x66 template write)
pub fn escape_fade_template() -> FadeTemplate {
    FadeTemplate {
        kind: 2,
        duration: 0x40,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        mode: [0, -1i16, 0],
    }
}

/// Live fade state, the engine mapping of the retail actor's `+0x7C` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeState {
    /// Fade kind (`state[0xc..]` as i32 in retail; template `[0]`).
    pub kind: i32,
    /// Current RGB, 10.6 fixed point.
    current_q6: [i16; 3],
    /// Target RGB, 10.6 fixed point.
    end_q6: [i16; 3],
    /// Per-frame delta, 10.6 fixed point.
    delta_q6: [i16; 3],
    /// Ramp duration in frames.
    pub duration: i16,
    /// Frames stepped so far.
    elapsed: i16,
    /// Mode words (template `[10..=12]`).
    pub mode: [i16; 3],
}

impl FadeState {
    /// Load a template into a live fade state, mirroring `FUN_80020B00`'s
    /// arithmetic exactly: start/end RGB promoted to 10.6 fixed point and the
    /// per-frame delta `((end - start) * 0x40) / duration` (i32 divide
    /// truncated to i16, as the retail store does).
    ///
    /// PORT: FUN_80020B00
    pub fn load(t: &FadeTemplate) -> FadeState {
        let duration = t.duration.max(1); // retail templates are never 0
        let mut current_q6 = [0i16; 3];
        let mut end_q6 = [0i16; 3];
        let mut delta_q6 = [0i16; 3];
        for c in 0..3 {
            current_q6[c] = t.start_rgb[c] << 6;
            end_q6[c] = t.end_rgb[c] << 6;
            delta_q6[c] =
                (((t.end_rgb[c] as i32 - t.start_rgb[c] as i32) * 0x40) / duration as i32) as i16;
        }
        FadeState {
            kind: t.kind as i32,
            current_q6,
            end_q6,
            delta_q6,
            duration,
            elapsed: 0,
            mode: t.mode,
        }
    }

    /// Advance the ramp one frame (the linear integrator the loader's
    /// state layout implies: `current += delta`, latching exactly on the
    /// target at the end of the ramp). Returns `true` while the fade is
    /// still running, `false` once it has completed. The retail pool
    /// actor's per-frame tick isn't dumped yet, so the latch-at-end is the
    /// engine's well-defined endpoint rather than a verified retail detail.
    pub fn step(&mut self) -> bool {
        if self.elapsed >= self.duration {
            return false;
        }
        self.elapsed += 1;
        if self.elapsed >= self.duration {
            self.current_q6 = self.end_q6;
            return false;
        }
        for c in 0..3 {
            self.current_q6[c] = self.current_q6[c].wrapping_add(self.delta_q6[c]);
        }
        true
    }

    /// The current display colour (`current >> 6`, clamped to a byte).
    pub fn rgb(&self) -> [u8; 3] {
        [
            (self.current_q6[0] >> 6).clamp(0, 255) as u8,
            (self.current_q6[1] >> 6).clamp(0, 255) as u8,
            (self.current_q6[2] >> 6).clamp(0, 255) as u8,
        ]
    }

    /// `true` once the ramp has run its full duration.
    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Ramp progress in `0.0..=1.0` (for hosts that drive an overlay alpha).
    pub fn progress(&self) -> f32 {
        self.elapsed as f32 / self.duration.max(1) as f32
    }
}

/// Fade-actor spawn wrapper - clean-room port of `FUN_80024E80` (`see
/// ghidra/scripts/funcs/80024e80.txt`), the most-cited helper in the dump
/// corpus: every subsystem that stages a full-screen fade goes through it.
///
/// Retail body: allocate a slot from the system-actor pool
/// (`actor_free(&DAT_80070674, _DAT_8007C34C)` - the generic effect-actor
/// list), and only on success stamp the caller's id into the template's
/// last word (`*(u16 *)(template + 0x18) = id`, i.e. i16 index 12 =
/// [`FadeTemplate::mode`]`[2]`) and run the loader ([`FadeState::load`] =
/// `FUN_80020B00`) on the actor's `+0x7C` block. Pool exhaustion returns 0
/// without touching the template.
///
/// The clean-room engine has no fixed-capacity fade-actor pool; `slot_free`
/// models the retail alloc outcome for hosts that cap concurrent fades
/// (pass `true` when a slot is available). The template is copied rather
/// than mutated in place - retail stamps a scratch buffer (e.g. the
/// battle-escape template at `DAT_801C9070`) that callers rebuild before
/// every spawn, so the copy is semantics-preserving.
///
/// PORT: FUN_80024E80
pub fn spawn_fade(template: &FadeTemplate, id: i16, slot_free: bool) -> Option<FadeState> {
    if !slot_free {
        // Retail `iVar1 == 0` branch: no stamp, no load.
        return None;
    }
    let mut t = *template;
    t.mode[2] = id;
    Some(FadeState::load(&t))
}

/// A persistent full-scene colour grade - the warm gold/sepia the opening
/// prologue cutscene (`opdeene`, "It was the Seru.") renders its whole 3D
/// scene through, distinct from the transient [`ColorFade`] flashes.
///
/// ## Retail mechanism
///
/// The grade is two GTE-lighting halves set for the cutscene scene and reset
/// for the interactive field:
/// - a **dim neutral ambient/back colour** (`0x8007B788` = `0x00202020`, i.e.
///   R=G=B=32/255 ≈ ⅛, vs `0x00FFFFFF` white in `town01`), staged into GTE
///   `RBK/GBK/BBK` (cr13-15) by `FUN_80043390`; and
/// - a **gold far-colour depth-cue tint** applied per render-node
///   (`+0x74` → GTE far colour cr21-23 in the TMD renderer `FUN_8002735C`,
///   written by the actor-VM colour opcode `0x0C`), which crushes the blue
///   channel to ~40% with R≈G.
///
/// Retail draw-list measurement (recomp GP0 capture of the opening chain):
/// lit gouraud + modulated-texture prims carry colour words ≈ `255:240:110`
/// (G/R ≈ 0.94, B/R ≈ 0.43), consistent across `opdeene` and `opurud`, while
/// bulk backdrop textures draw at *neutral* `0x808080` modulation - their
/// amber is pre-baked in the texels and retail preserves its chroma. The
/// engine reproduces the mechanism with a per-channel **multiply tint** (see
/// `engine-render` `apply_grade`): each shaded pixel becomes `rgb * gold`
/// cross-faded by `strength`, which crushes blue on neutral content and
/// leaves the warm backdrop chroma intact, exactly as retail's modulation
/// multiply does. Whole-frame framebuffer average of the retail cutscene is
/// RGB `(61, 55, 15)` (G/R ≈ 0.90, B/R ≈ 0.24) - warmer than the tint alone
/// because the already-amber texels multiply under it.
///
/// REF: FUN_80043390 (ambient → GTE cr13-15)
/// REF: FUN_8002735C (far colour → GTE cr21-23)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorGrade {
    /// Per-channel multiply tint applied to the shaded pixel, `0.0..=1.0`
    /// per channel, in the same display-referred space as every other colour
    /// the engine handles (PSX framebuffer values; nothing re-encodes them).
    pub gold: [f32; 3],
    /// Cross-fade strength `0.0..=1.0` (`0` = untouched, `1` = full tint).
    pub strength: f32,
}

impl ColorGrade {
    /// The `opdeene` opening-prologue grade: the multiply tint is the retail
    /// draw-list *modulation* ratio `255:240:110` → `(1.0, 0.94, 0.43)` -
    /// the depth-blended amber actually baked into drawn geometry, not the
    /// GTE far-colour extreme `255:230:62` only the farthest verts reach.
    ///
    /// Colour-space derivation: retail multiplies this tint in PSX integer
    /// space, i.e. on display-referred framebuffer values. The engine's
    /// shaded pixel is the same display-referred value and the render
    /// attachment is always viewed UNORM (never sRGB - pinned by
    /// `engine-render` `tests::color_space`), so nothing re-encodes the
    /// product: the stored coefficients are retail's measured display
    /// ratios verbatim, no gamma adjustment. Full strength (`1.0`).
    ///
    /// Pixel check against the retail `opdeene` tableau framebuffer: on the
    /// matched gold-geometry regions (Seru spires / rock spires / sky) the
    /// tinted engine output lands G/R 0.91..0.93 vs retail's ~0.89, and the
    /// near-field surface B/R ~0.37 sits against retail's near-ground 0.44
    /// (the beat where the modulation ratio shows almost unblended). The
    /// far-field crush (retail sky/spire B/R down to 0.12..0.18) is the
    /// per-render-node DPCS depth-cue pull toward the gold far colour
    /// (`+0x74`/`+0x78`, GTE cr21-23), which a uniform multiply cannot
    /// reproduce - the engine stages it separately as
    /// [`DepthCueRamp::PROLOGUE_GOLD`], layered over this tint.
    pub const PROLOGUE_SEPIA: ColorGrade = ColorGrade {
        gold: [1.0, 0.94, 0.43],
        strength: 1.0,
    };
}

/// The prologue's **per-render-node depth-cue pull** - the second half of the
/// retail grade, layered over [`ColorGrade::PROLOGUE_SEPIA`]'s multiply tint.
///
/// ## Retail mechanism
///
/// The TMD renderer `FUN_8002735C` runs the GTE **DPCS** depth cue per prim:
/// `out = base + IR0 * (far - base)`, with the far colour and `IR0` staged
/// **per render node** (`+0x74` → GTE cr21-23, `+0x78` → `IR0`). Across the
/// opening's narration beats the cutscene host stages a gold far colour with
/// depth-graded `IR0`s, so far scenery (sky planes, distant spires) crushes
/// hard toward gold (retail far-field `B/R ≈ 0.12..0.18`) while near ground
/// keeps the modulation tint almost unblended (`B/R ≈ 0.44`). DPCS runs on
/// the *packet colour* before the GPU's texel multiply, so on textured prims
/// the far term reaches the pixel as `texel * far / 128` - texture detail
/// survives the crush, as the retail framebuffer shows.
///
/// The engine reproduces the depth dependence as a linear view-depth ramp:
/// `ir0(z) = clamp((z - near_z) / (far_z - near_z), 0, 1) * max_ir0` per
/// fragment (`engine-render` `cue_ramp_ir0`), staged with
/// `Renderer::set_depth_cue_ramp` while this ramp is active and cleared
/// otherwise - interactive scenes (`town01` onward) render with the ramp off,
/// which is the bit-identical pre-ramp path.
///
/// REF: FUN_8002735C (far colour / IR0 → GTE cr21-23 / IR0)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthCueRamp {
    /// DPCS far colour (GTE cr21-23) in display `0..1` per channel.
    pub far: [f32; 3],
    /// View depth (camera units) where the pull begins (`ir0 = 0`).
    pub near_z: f32,
    /// View depth of the full pull (`ir0 = max_ir0`).
    pub far_z: f32,
    /// `IR0` ceiling, `0.0..=1.0` (hardware `0..0x1000`).
    pub max_ir0: f32,
}

impl DepthCueRamp {
    /// The opening-prologue gold pull, calibrated pixel-for-pixel against the
    /// retail tableau framebuffer on matched regions: the far cave wall lands
    /// within a few percent of retail per channel with `B/R` inside retail's
    /// `0.12..0.18` far-field band, the gold spires hold `G/R ≈ 0.87..0.90`
    /// (retail `~0.89`), and the near ground stays on the unpulled multiply
    /// tint. The `near_z`/`far_z` window sits just past the cutscene camera's
    /// telephoto ground plane (the eye is ~3.5k view units out of the
    /// tableau, so the whole depth spread is only a few hundred units); the
    /// far colour keeps the modulation ratio's gold hue at roughly an eighth
    /// of full brightness - the retail wall's effective post-pull modulation.
    /// Retail's true staging is per node, which a shared ramp cannot fully
    /// reproduce: the spire nodes combine a strong pull with a brighter far
    /// colour, so their `B/R` keeps a documented residual (see
    /// `docs/subsystems/cutscene.md`).
    pub const PROLOGUE_GOLD: DepthCueRamp = DepthCueRamp {
        far: [0.121, 0.111, 0.0095],
        near_z: 3250.0,
        far_z: 3550.0,
        max_ir0: 0.92,
    };
}

/// Field-VM colour-fade overlay (op `0x34` sub-0, `FUN_801E1FB0`).
///
/// The field/cutscene fade path: a full-screen wash of one colour whose
/// *coverage* ramps over a short window. Unlike [`FadeState`] (which ramps the
/// RGB channels for the battle escape white-out), this holds a fixed colour
/// and ramps how much of the screen it covers - the shape the opening
/// prologue's white flash needs (`34 05 FF FF FF 00 00` = a white overlay that
/// fades to reveal the scene).
///
/// ## Approximate by design
///
/// The retail fade actor's per-frame draw handler is not dumped, so the exact
/// coverage curve + PSX blend mode are not pinned. This models the documented
/// setup (`FUN_801E1FB0`: colour = operand RGB, direction from `op0 & 1`,
/// zero-colour = clear) as a linear coverage ramp; the host draws it with a
/// semi-transparent wash. When the draw handler is dumped this can be made
/// byte-exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorFade {
    /// Overlay colour (operand RGB).
    pub rgb: [u8; 3],
    /// Ramp length in frames.
    pub frames: u16,
    /// Frames stepped so far.
    elapsed: u16,
    /// When `true`, coverage ramps 1.0 → 0.0 (a fade-*from*-colour reveal, the
    /// opening white flash); when `false`, 0.0 → 1.0 (a fade-*to*-colour).
    pub reveal: bool,
}

impl ColorFade {
    /// Default ramp length for a field colour fade (frames). Retail flashes
    /// are brief; the exact count is per-op and not pinned, so this is a
    /// reasonable opening-flash duration (~0.5 s at 60 fps).
    pub const DEFAULT_FRAMES: u16 = 32;

    /// Build a colour fade from an op-`0x34` sub-0 setup: `op0`'s low bit
    /// selects direction (`& 1` = reveal / fade-from-colour, the opening
    /// flash form), the RGB is the wash colour. A zero colour is *not* a fade;
    /// callers clear the overlay instead (mirrors retail's "all colour bytes
    /// zero → clear `_DAT_8007B62C`").
    ///
    /// REF: FUN_801E1FB0
    pub fn from_op34(op0: u8, rgb: [u8; 3]) -> ColorFade {
        ColorFade {
            rgb,
            frames: Self::DEFAULT_FRAMES,
            elapsed: 0,
            reveal: op0 & 1 != 0,
        }
    }

    /// Advance one frame. Returns `true` while still running, `false` once the
    /// ramp completes (the host then drops the overlay).
    pub fn step(&mut self) -> bool {
        if self.elapsed >= self.frames {
            return false;
        }
        self.elapsed += 1;
        self.elapsed < self.frames
    }

    /// Screen coverage in `0.0..=1.0` this frame: `reveal` ramps down from
    /// full, otherwise up from empty.
    pub fn coverage(&self) -> f32 {
        let p = self.elapsed as f32 / self.frames.max(1) as f32;
        if self.reveal { 1.0 - p } else { p }
    }

    /// The wash colour.
    pub fn rgb(&self) -> [u8; 3] {
        self.rgb
    }

    /// `true` once the ramp has completed.
    pub fn finished(&self) -> bool {
        self.elapsed >= self.frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prologue_sepia_is_a_warm_gold_multiply_tint() {
        let g = ColorGrade::PROLOGUE_SEPIA;
        assert_eq!(g.strength, 1.0, "full tint");
        assert_eq!(g.gold[0], 1.0, "red is the anchor channel");
        assert!(g.gold[1] < g.gold[0], "green below red");
        assert!(g.gold[2] < g.gold[1], "blue crushed below green");
        // Display-referred, like every colour the engine handles: nothing on
        // the path re-encodes the shaded pixel (the attachment is UNORM), so
        // the stored coefficients are retail's measured on-geometry
        // modulation ratio as-is - 255:240:110, G/R ~= 0.94, B/R ~= 0.43.
        let disp_g = g.gold[1] / g.gold[0];
        let disp_b = g.gold[2] / g.gold[0];
        assert!(
            (disp_g - 0.94).abs() < 0.02,
            "display G/R ~= 0.94, got {disp_g:.3}"
        );
        assert!(
            (disp_b - 0.43).abs() < 0.02,
            "display B/R ~= 0.43, got {disp_b:.3}"
        );
    }

    #[test]
    fn color_fade_reveal_ramps_coverage_down() {
        // op0 = 5 (low bit set) = reveal: coverage starts full, ends empty.
        let mut f = ColorFade::from_op34(0x05, [0xFF, 0xFF, 0xFF]);
        assert!(f.reveal);
        assert_eq!(f.rgb(), [0xFF, 0xFF, 0xFF]);
        assert_eq!(f.coverage(), 1.0);
        let mut frames = 0;
        while f.step() {
            frames += 1;
        }
        assert_eq!(frames + 1, ColorFade::DEFAULT_FRAMES as i32);
        assert!(f.finished());
        assert_eq!(f.coverage(), 0.0, "reveal lands fully transparent");
    }

    #[test]
    fn color_fade_cover_ramps_coverage_up() {
        // op0 = 4 (low bit clear) = fade-to-colour.
        let mut f = ColorFade::from_op34(0x04, [0, 0, 0]);
        assert!(!f.reveal);
        assert_eq!(f.coverage(), 0.0);
        while f.step() {}
        assert_eq!(f.coverage(), 1.0, "cover lands fully opaque");
    }

    #[test]
    fn loader_matches_the_retail_fixed_point_layout() {
        // start 0x20, end 0xFF over 0x40 frames: delta = (0xDF * 0x40)/0x40
        // = 0xDF in 10.6 - i.e. (end-start)/duration per displayed unit.
        let t = FadeTemplate {
            kind: 2,
            duration: 0x40,
            start_rgb: [0x20, 0x20, 0x20],
            end_rgb: [0xFF, 0xFF, 0xFF],
            mode: [0, 0, 0],
        };
        let f = FadeState::load(&t);
        assert_eq!(f.kind, 2);
        assert_eq!(f.rgb(), [0x20, 0x20, 0x20]);
        assert_eq!(f.delta_q6[0], ((0xFF - 0x20) * 0x40) / 0x40);
    }

    #[test]
    fn escape_fade_ramps_black_to_white_over_0x40_frames() {
        let mut f = FadeState::load(&escape_fade_template());
        assert_eq!(f.rgb(), [0, 0, 0]);
        assert_eq!(f.duration, 0x40);
        let mut frames = 0;
        while f.step() {
            frames += 1;
        }
        assert_eq!(frames + 1, 0x40, "ramp runs the template duration");
        assert!(f.finished());
        assert_eq!(f.rgb(), [0xFF, 0xFF, 0xFF], "lands exactly on white");
    }

    #[test]
    fn spawn_fade_stamps_id_into_the_last_template_word() {
        // FUN_80024E80: `*(u16*)(template + 0x18) = id` before the loader
        // runs - byte offset 0x18 = i16 index 12 = mode[2]. The loader
        // copies template[12] onto the state (retail state word 0x11).
        let t = escape_fade_template();
        let f = spawn_fade(&t, 0x1234, true).expect("slot free");
        assert_eq!(f.mode, [0, -1i16, 0x1234], "id lands in mode[2] only");
        // Everything else matches a plain load of the same template.
        let plain = FadeState::load(&t);
        assert_eq!(f.kind, plain.kind);
        assert_eq!(f.duration, plain.duration);
        assert_eq!(f.rgb(), plain.rgb());
    }

    #[test]
    fn spawn_fade_pool_exhausted_returns_none() {
        // Retail `iVar1 == 0` branch: alloc failed, nothing stamped/loaded.
        assert_eq!(spawn_fade(&escape_fade_template(), 7, false), None);
    }

    #[test]
    fn spawn_fade_does_not_mutate_the_caller_template() {
        let t = escape_fade_template();
        let _ = spawn_fade(&t, 0x7FFF, true);
        assert_eq!(t.mode, [0, -1i16, 0], "caller copy untouched");
    }

    #[test]
    fn midpoint_is_linear() {
        let mut f = FadeState::load(&escape_fade_template());
        for _ in 0..0x20 {
            f.step();
        }
        let [r, ..] = f.rgb();
        // 0xFF*0x40/0x40 per frame in q6: after 32 frames ≈ 127.
        assert!((126..=128).contains(&r), "halfway ≈ mid grey, got {r}");
    }
}
