//! The field-side seeds a capture carries - a scene's placed-object clips,
//! ambient walkers, morph envelopes, fog pool, sprite arms, CLUT-cell
//! cyclers, scroller rects, panel widget and camera block - as records with
//! the environment form the image child reads them back from.

use legaia_engine_core::world::SeededVramRect;

/// `retail_panel` as `LEGAIA_SEAT_PANEL`: the widget's fields as
/// comma-separated integers in [`panel_from_env`]'s order.
pub fn panel_env(p: &legaia_engine_effects::screen_fx::PanelWidget) -> String {
    let mut v: Vec<i32> = Vec::new();
    v.extend(p.cur.iter().map(|&x| i32::from(x)));
    v.extend(p.target.iter().map(|&x| i32::from(x)));
    v.extend(p.base.iter().map(|&x| i32::from(x)));
    v.extend([
        i32::from(p.t),
        i32::from(p.dur),
        i32::from(p.w0),
        i32::from(p.h0),
        i32::from(p.u),
        i32::from(p.v),
        i32::from(p.texpage),
        i32::from(p.texpage2),
    ]);
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Inverse of [`panel_env`].
pub fn panel_from_env(s: &str) -> Option<legaia_engine_effects::screen_fx::PanelWidget> {
    let v: Vec<i32> = s
        .split(',')
        .map(|t| t.trim().parse().ok())
        .collect::<Option<_>>()?;
    if v.len() != 21 {
        return None;
    }
    let h = |i: usize| v[i] as i16;
    Some(legaia_engine_effects::screen_fx::PanelWidget {
        cur: [h(0), h(1), h(2), h(3), h(4)],
        target: [h(5), h(6), h(7), h(8), h(9)],
        base: [h(10), h(11), h(12)],
        t: h(13),
        dur: h(14),
        w0: h(15),
        h0: h(16),
        u: v[17] as u8,
        v: v[18] as u8,
        texpage: v[19] as u16,
        texpage2: v[20] as u16,
    })
}

/// `retail_scroll_rects` as the bytes of a `LEGAIA_SEAT_VRAM_RECTS` file:
/// per rect `x, y, w, h` (`u16` LE) then its `w * h` texels.
pub fn vram_rects_file(rects: &[SeededVramRect]) -> Vec<u8> {
    let mut out = Vec::new();
    for ((x, y, w, h), texels) in rects {
        for v in [*x, *y, *w, *h].iter().chain(texels) {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// Inverse of [`vram_rects_file`]; a truncated tail is dropped.
pub fn vram_rects_from_file(bytes: &[u8]) -> Vec<SeededVramRect> {
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= words.len() {
        let (x, y, w, h) = (words[i], words[i + 1], words[i + 2], words[i + 3]);
        let n = usize::from(w) * usize::from(h);
        let Some(texels) = words.get(i + 4..i + 4 + n) else {
            break;
        };
        out.push(((x, y, w, h), texels.to_vec()));
        i += 4 + n;
    }
    out
}

/// One drawn field actor's clip words: record `+0x50`, clip id `+0x5C`,
/// cursor `+0x68`, control word `+0x62` and cursor step `+0x6A` - what the
/// anim tick `FUN_800204F8` steps and the draw walker poses from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectClipSeed {
    pub record: u16,
    pub clip: u8,
    pub cursor: i16,
    pub flags: u16,
    pub rate: i16,
}

/// `LEGAIA_SEAT_OBJECT_CLIPS`: `record:clip:cursor:flags:rate`, `;`-joined.
pub fn object_clips_env(clips: &[ObjectClipSeed]) -> String {
    clips
        .iter()
        .map(|c| {
            format!(
                "{}:{}:{}:{}:{}",
                c.record, c.clip, c.cursor, c.flags, c.rate
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`object_clips_env`]; a malformed entry is dropped.
pub fn object_clips_from_env(v: &str) -> Vec<ObjectClipSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.split(':').map(str::trim);
            Some(ObjectClipSeed {
                record: f.next()?.parse().ok()?,
                clip: f.next()?.parse().ok()?,
                cursor: f.next()?.parse().ok()?,
                flags: f.next()?.parse().ok()?,
                rate: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// One ambient walker's live seat: flat MAN index `+0x50`, `+0x14` /
/// `+0x18`, and the retail-space heading `+0x26`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkerSeed {
    pub flat: u16,
    pub x: i16,
    pub z: i16,
    pub heading: u16,
}

/// `retail_walkers` as `LEGAIA_SEAT_WALKERS`: `flat:x:z:heading` per
/// walker, `;`-joined, decimal.
pub fn walkers_env(w: &[WalkerSeed]) -> String {
    w.iter()
        .map(|s| format!("{}:{}:{}:{}", s.flat, s.x, s.z, s.heading))
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`walkers_env`].
pub fn walkers_from_env(v: &str) -> Vec<WalkerSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            Some(WalkerSeed {
                flat: f.next()?.parse().ok()?,
                x: f.next()?.parse().ok()?,
                z: f.next()?.parse().ok()?,
                heading: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// One field actor's live VDF morph envelope (op `0x4B`,
/// `legaia_engine_core::world::npc_morph`): its flat MAN index `+0x50`, the
/// lane weights `+0xA0 + i*2` over its `+0x6C` lanes, the lane-done mask
/// `+0x7C` and the envelope control word `+0x62`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphSeed {
    pub flat: u16,
    pub weights: Vec<u16>,
    pub done_mask: u32,
    pub env: u16,
}

/// `retail_morphs` as `LEGAIA_SEAT_MORPHS`:
/// `flat:w0/w1/..:done:env` per actor, `;`-joined, numbers in hex.
pub fn morphs_env(m: &[MorphSeed]) -> String {
    m.iter()
        .map(|s| {
            format!(
                "{:x}:{}:{:x}:{:x}",
                s.flat,
                s.weights
                    .iter()
                    .map(|w| format!("{w:x}"))
                    .collect::<Vec<_>>()
                    .join("/"),
                s.done_mask,
                s.env
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`morphs_env`].
pub fn morphs_from_env(v: &str) -> Vec<MorphSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            let flat = u16::from_str_radix(f.next()?, 16).ok()?;
            let weights = f
                .next()?
                .split('/')
                .map(|w| u16::from_str_radix(w, 16).ok())
                .collect::<Option<Vec<u16>>>()?;
            let done_mask = u32::from_str_radix(f.next()?, 16).ok()?;
            let env = u16::from_str_radix(f.next()?, 16).ok()?;
            Some(MorphSeed {
                flat,
                weights,
                done_mask,
                env,
            })
        })
        .collect()
}

/// `retail_fog` as `LEGAIA_SEAT_FOG`: `slot,age,rate,vx,vz,x,z,y,grey` per
/// record, `;`-separated.
pub fn fog_env(fog: &[legaia_engine_field::fog_particles::FogParticle]) -> String {
    fog.iter()
        .map(|p| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                p.slot, p.age, p.rate, p.vx, p.vz, p.x, p.z, p.y, p.grey
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`fog_env`]; malformed entries are dropped.
pub fn fog_from_env(s: &str) -> Vec<legaia_engine_field::fog_particles::FogParticle> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i64> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [slot, age, rate, vx, vz, x, z, y, grey] = <[i64; 9]>::try_from(v).ok()?;
            Some(legaia_engine_field::fog_particles::FogParticle {
                age: age as u16,
                rate: rate as u16,
                slot: slot as u8,
                alive: true,
                vx: vx as i8,
                vz: vz as i8,
                x: x as i32,
                z: z as i32,
                y: y as i16,
                grey: grey as u8,
            })
        })
        .collect()
}

/// `retail_sprite_arms` as `LEGAIA_SEAT_SPRITE_ARMS`:
/// `record,x,y,z,r24,r26,r28,scale,colour,level` per sheet, `;`-separated.
pub fn sprite_arms_env(s: &[legaia_engine_core::world::ambient::SpriteArmSeed]) -> String {
    s.iter()
        .map(|a| {
            let pose: String = a
                .pose
                .iter()
                .flat_map(|e| e.iter().map(|b| format!("{b:02x}")))
                .collect();
            format!(
                "{},{},{},{},{},{},{},{},{},{},{pose}",
                a.record_off,
                a.pos[0],
                a.pos[1],
                a.pos[2],
                a.rot[0],
                a.rot[1],
                a.rot[2],
                a.scale,
                a.colour,
                a.level
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`sprite_arms_env`]; malformed entries are dropped.
pub fn sprite_arms_from_env(s: &str) -> Vec<legaia_engine_core::world::ambient::SpriteArmSeed> {
    s.split(';')
        .filter_map(|e| {
            let mut fields: Vec<&str> = e.split(',').collect();
            let pose_hex = if fields.len() == 11 {
                fields.pop()?
            } else {
                ""
            };
            let v: Vec<i64> = fields
                .iter()
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            let [rec, x, y, z, r0, r1, r2, scale, colour, level] = <[i64; 10]>::try_from(v).ok()?;
            let bytes: Vec<u8> = (0..pose_hex.len() / 2)
                .filter_map(|i| u8::from_str_radix(pose_hex.get(i * 2..i * 2 + 2)?, 16).ok())
                .collect();
            Some(legaia_engine_core::world::ambient::SpriteArmSeed {
                record_off: rec as u32,
                pos: [x as i16, y as i16, z as i16],
                rot: [r0 as i16, r1 as i16, r2 as i16],
                scale: scale as u16,
                colour: colour as u32,
                level: level as u16,
                pose: bytes.as_chunks::<8>().0.to_vec(),
            })
        })
        .collect()
}

/// `retail_cell_fx` as `LEGAIA_SEAT_CLUT_FX`: `x,y,w,h,h,s,v,mode,white`
/// per part, `;`-separated.
pub fn cell_fx_env(fx: &[legaia_engine_field::clut_cell_fx::ClutCellFx]) -> String {
    fx.iter()
        .map(|f| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                f.rect.0, f.rect.1, f.rect.2, f.rect.3, f.h_add, f.s_add, f.v_add, f.mode, f.white
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`cell_fx_env`]; malformed entries are dropped.
pub fn cell_fx_from_env(s: &str) -> Vec<legaia_engine_field::clut_cell_fx::ClutCellFx> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i32> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [x, y, w, h, hh, ss, vv, mode, white] = <[i32; 9]>::try_from(v).ok()?;
            Some(legaia_engine_field::clut_cell_fx::ClutCellFx {
                rect: (x as u16, y as u16, w as u16, h as u16),
                h_add: hh as i16,
                s_add: ss as i16,
                v_add: vv as i16,
                mode: mode as i16,
                white: white as i16,
            })
        })
        .collect()
}

/// `LEGAIA_SEAT_CAMERA_BLOCK` for `play-window`: the block's 40 bytes from
/// `CAMERA_BLOCK`, hex.
pub fn camera_block_env(block: &legaia_engine_field::camera_zone::CameraZoneConfig) -> String {
    block
        .to_retail_block()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Inverse of [`camera_block_env`].
pub fn camera_block_from_env(
    s: &str,
) -> Option<legaia_engine_field::camera_zone::CameraZoneConfig> {
    let s = s.trim();
    if s.len() != 0x50 {
        return None;
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    legaia_engine_field::camera_zone::CameraZoneConfig::from_retail_block(&bytes)
}

#[cfg(test)]
mod tests;
