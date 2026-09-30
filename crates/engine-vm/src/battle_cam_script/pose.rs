//! Camera poses: dialogue, boot and menu framings, plus the actor / formation inputs they read.
//! Split out of `battle_cam_script.rs`.

/// One camera pose: 12-bit angle units (`4096` = full turn) + the eye-space
/// translation trio, exactly the retail globals' value space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleCamPose {
    /// Pitch, 12-bit units (`0x8007B790`).
    pub pitch: f32,
    /// Yaw, 12-bit units (`0x8007B792`). May run outside `[0, 4096)`
    /// mid-glide (shortest-arc unwrap); normalized when the orbit owns it.
    pub yaw: f32,
    /// Eye-space translation `(x, y, z)` (`0x800840B8/BC/C0`).
    pub tr: [f32; 3],
    /// The world point the camera orbits (`0x80089118/1C/20`, stored negated
    /// in retail; held un-negated here). `FUN_801D829C` tweens it on the same
    /// clock as the rotation and translation trios.
    pub focus: [f32; 3],
}

/// Tutorial-dialogue close-up (trace frames 1..45: 240+ frames static),
/// before its focus is filled in by [`dialogue_pose`].
pub(super) const DIALOGUE_POSE: BattleCamPose = BattleCamPose {
    pitch: 0.0,
    yaw: 0.0,
    tr: [0.0, 1280.0, 1638.0],
    focus: [0.0; 3],
};

/// The monster seat the dialogue close-up frames when a host has no
/// formation to read it from: the retail solo-fight seat `(0, 800)`.
pub(super) const DIALOGUE_FOCUS_FALLBACK: [f32; 3] = [0.0, 0.0, 800.0];

/// The tutorial-dialogue close-up for a formation. The focus is the
/// speaking monster's seat, not the formation centre: both Tetsu-tutorial
/// captures (`v0_1_battle_start_tetsu`, `s5_tetsu_battle`) read the focus
/// trio `0x80089118` as the negated `(0, 0, 800)` - Tetsu's `+0x34/+0x38` -
/// under `TR (0, 1280, 1638)`, so the eye sits `prescale(0x400)` in front of
/// his face. The monster row is the formation's far-Z edge (party seats at
/// `-Z`, monsters at `+Z`, [`crate::battle_seats`]-style), centred in X.
pub fn dialogue_pose(formation: Option<FormationBox>) -> BattleCamPose {
    BattleCamPose {
        focus: formation
            .map(|b| [(b.min[0] + b.max[0]) * 0.5, 0.0, b.max[1]])
            .unwrap_or(DIALOGUE_FOCUS_FALLBACK),
        ..DIALOGUE_POSE
    }
}
/// Far Begin/Run framing, `FUN_801D5854` case `9`. Pitch and TR.x / TR.y are
/// the case's constants; yaw free-orbits and TR.z is formation-sized.
pub(super) const MENU_PITCH: f32 = 32.0; // 0x20
pub(super) const MENU_TR_X: f32 = 0.0;
pub(super) const MENU_TR_Y: f32 = 1280.0; // 0x500
/// Formation span -> raw eye-space depth: `max(dx, dz) * 3`, floored.
pub(super) const MENU_SPAN_SCALE: f32 = 3.0;
pub(super) const MENU_TR_Z_MIN_RAW: f32 = 2048.0; // 0x800
/// Submenu close-up constants, from `FUN_801D5854` case `0`.
pub(super) const SUBMENU_PITCH: f32 = 32.0; // 0x20
/// Yaw base: retail computes `0x8F0 - actor_facing`.
pub(super) const SUBMENU_YAW_BASE: i32 = 0x8F0; // 2288
/// Eye-space X, constant across every seat and character.
pub(super) const SUBMENU_TR_X: f32 = -512.0; // -0x200
/// Raw eye-space Z before `FUN_801D829C`'s projection prescale.
pub(super) const SUBMENU_TR_Z_RAW: i32 = 0x600; // 1536

/// The pose a host renders before the first camera tick arms the state: the
/// far framing at its minimum depth (`prescale(0x800)`), on the origin.
pub const BOOT_POSE: BattleCamPose = BattleCamPose {
    pitch: MENU_PITCH,
    yaw: 0.0,
    tr: [MENU_TR_X, MENU_TR_Y, prescale_tr_z(0x800)],
    focus: [0.0; 3],
};

/// `FUN_801D829C`'s TR.z prescale: world distance -> GTE projection units.
/// `0xA0` = 160 = PSX screen half-width; `<< 8` = GTE `H = 256`. The divide
/// truncates, which is why the traced `0x600` lands on `2457`, not `2457.6`.
pub const fn prescale_tr_z(raw: i32) -> f32 {
    ((raw << 8) / 0xA0) as f32
}

/// Camera height used when the host has no disc table to resolve
/// `0x801F4D2C` from. Vahn's entry, the one value the solo-Vahn camera trace
/// observes, so an unpinned character frames like the measured case instead
/// of jumping. Real per-character heights come from
/// `legaia_asset::battle_camera_table` via [`BattleCamActor::height`].
pub const SUBMENU_HEIGHT_FALLBACK: f32 = 1152.0; // 0x480

/// The acting battle actor the submenu framing is built around.
///
/// `facing` is retail `actor[+0x46]` (12-bit angle), `world` the actor
/// position at `actor[+0x34/+0x36/+0x38]`, and `height` the `TR.y` the host
/// resolved out of the disc table `0x801F4D2C` for this actor's character id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleCamActor {
    pub facing: i32,
    pub world: [f32; 3],
    /// Per-character `TR.y` from `0x801F4D2C`; `None` falls back to
    /// [`SUBMENU_HEIGHT_FALLBACK`].
    pub height: Option<f32>,
}

impl Default for BattleCamActor {
    /// The measured solo-Vahn case: facing `0`, seated at the traced
    /// `(0, 0, -800)`, on the fallback height. Reproduces the originally
    /// pinned framing exactly, so an un-wired host keeps the measured
    /// behaviour.
    fn default() -> Self {
        BattleCamActor {
            facing: 0,
            world: [0.0, 0.0, -800.0],
            height: None,
        }
    }
}

impl BattleCamActor {
    /// Retail's case-0 submenu framing for this actor (`FUN_801D5854`): a
    /// fixed over-the-shoulder offset, facing-relative yaw, per-character
    /// height, orbiting the actor's own position.
    pub fn submenu_pose(self) -> BattleCamPose {
        BattleCamPose {
            pitch: SUBMENU_PITCH,
            yaw: (SUBMENU_YAW_BASE - self.facing).rem_euclid(4096) as f32,
            tr: [
                SUBMENU_TR_X,
                self.height.unwrap_or(SUBMENU_HEIGHT_FALLBACK),
                prescale_tr_z(SUBMENU_TR_Z_RAW),
            ],
            focus: self.world,
        }
    }
}

/// The world-space X/Z extent of the actors the far framing encloses -
/// retail's `min`/`max` walk over `actor[+0x34]` / `actor[+0x38]` for every
/// present actor in the selected slot range (`FUN_801D5854` case `9`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationBox {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl FormationBox {
    /// Fold one present actor's world X/Z into the accumulator - retail's
    /// per-actor `min`/`max` step, shared so both hosts build the box with
    /// the same arithmetic (only the host-side presence predicate differs).
    pub fn extend(bbox: &mut Option<FormationBox>, x: f32, z: f32) {
        match bbox {
            None => {
                *bbox = Some(FormationBox {
                    min: [x, z],
                    max: [x, z],
                })
            }
            Some(b) => {
                b.min[0] = b.min[0].min(x);
                b.min[1] = b.min[1].min(z);
                b.max[0] = b.max[0].max(x);
                b.max[1] = b.max[1].max(z);
            }
        }
    }
}

/// The far framing's eye-space Z in **raw world units**, before the
/// projection prescale: `max(span * 3, 0x800)`, where `span` is the larger of
/// the formation's two extents. This is the value retail's case 9 hands the
/// tween builder, and [`prescale_tr_z`] is what the builder then does to it.
pub fn menu_raw_z(bbox: Option<FormationBox>) -> i32 {
    let span = match bbox {
        // Retail keeps the LARGER of the two extents, so a formation that is
        // wide but shallow still fits the frame.
        Some(b) => (b.max[0] - b.min[0]).max(b.max[1] - b.min[1]),
        None => 0.0,
    };
    (span * MENU_SPAN_SCALE).max(MENU_TR_Z_MIN_RAW) as i32
}

/// Retail's case-9 far framing for a formation. `None` (no present actors)
/// keeps the minimum depth and the origin focus, which is what retail's
/// un-entered min/max accumulators degenerate to.
pub fn menu_framing(bbox: Option<FormationBox>, yaw: f32) -> BattleCamPose {
    let focus = match bbox {
        Some(b) => [
            (b.min[0] + b.max[0]) * 0.5,
            0.0,
            (b.min[1] + b.max[1]) * 0.5,
        ],
        None => [0.0; 3],
    };
    BattleCamPose {
        pitch: MENU_PITCH,
        yaw,
        tr: [MENU_TR_X, MENU_TR_Y, prescale_tr_z(menu_raw_z(bbox))],
        focus,
    }
}
