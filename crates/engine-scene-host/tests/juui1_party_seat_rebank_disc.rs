//! Disc-gated: a `4C 51` seat re-binds its clip even when it names the id
//! already playing, so a bank switch on the same id lands.
//!
//! `juui1` (Ra-Seru scene 1) seats the party for its return beat, `P2[2]`
//! (story flag `0x50F`), with `CC 09 51 68 4C 04 02`, `CC 0A 51 69 CA 04 05`
//! and `CC 0B 51 67 CB 04 09`: depth `04` clears each actor's party-bank bit,
//! and the ids name the scene bundle's flying clips. Vahn's spawn prologue
//! (`P1[2]`: `4C 50 F0`, `31 18`, `22 02`) leaves him on party-bank clip `2`,
//! his standing idle, so his seat names the id he is already playing. The
//! seat's NPC arm forces the restart (`+0x5E = -2` at `0x801E1A94` before
//! `jal 0x800204F8`), and the consumer reads the cleared bit, so retail binds
//! scene record `1`. A cue gated on the playing id drops there, and Vahn stood
//! through the beat while Noa and Gala flew.
//!
//! The test enters the scene with the flag raised, binds each party member's
//! spawn clip as a host does, ticks the beat with the hosts' cue drain, and
//! checks that all three end on their scene-bank flying record. Skip-passes
//! without `LEGAIA_DISC_BIN`.

use legaia_engine_field::field_anim::FieldClipPlayer;
use legaia_engine_scene_host::scene_host::SceneHost;

/// The beat's return-visit flag, which the entry script (`P1[0]`) tests
/// before it spawns `P2[2]`.
const RETURN_BEAT_FLAG: u16 = 0x50F;

/// `(placement slot, scene clip id)` of each party seat in `P2[2]`.
const SEATS: [(u8, u8); 3] = [(2, 2), (3, 5), (4, 9)];

#[test]
fn juui1_return_seat_flies_vahn_off_the_scene_bank() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    let loco = legaia_actor_formats::character_pack::field_locomotion_anm(
        &host
            .index
            .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
            .expect("PROT 0874"),
    )
    .expect("party locomotion bundle");
    host.world.system_flag_set(RETURN_BEAT_FLAG);
    host.enter_field_scene("juui1", 0).expect("enter juui1");
    let scene_anm =
        legaia_engine_scene::npc_catalog::scene_anm_bundle(host.scene.as_ref().unwrap())
            .expect("juui1 scene clip bank");

    // What a host does at scene entry: bind each party member's spawn clip
    // out of the bank its live bit names.
    for (slot, _) in SEATS {
        let anim = host
            .world
            .field_npc_live_anim(usize::from(slot))
            .expect("party placement carries a clip");
        let party = host.world.npc_party_bank(slot).unwrap_or(true);
        let bank = if party { &loco } else { &scene_anm };
        let p = FieldClipPlayer::from_record(bank, usize::from(anim.max(1)) - 1)
            .expect("spawn clip decodes");
        host.world.bind_npc_clip_cursor(slot, anim, &p);
    }

    // The last clip each seat's slot was re-targeted to.
    let mut bound: std::collections::HashMap<u8, (usize, usize)> = Default::default();
    for _ in 0..900 {
        let out = host
            .world
            .drain_field_anim_cues(Some(&scene_anm), Some(&loco), |s| {
                Some(SEATS.iter().any(|&(slot, _)| slot == s))
            });
        for r in out {
            bound.insert(r.slot, (r.player.frame_count(), r.player.bone_count()));
        }
        let seated = SEATS
            .iter()
            .all(|&(slot, _)| host.world.npc_party_bank(slot) == Some(false));
        if seated && SEATS.iter().all(|(s, _)| bound.contains_key(s)) {
            break;
        }
        let _ = host.tick();
    }

    for (slot, clip) in SEATS {
        assert_eq!(
            host.world.npc_party_bank(slot),
            Some(false),
            "P1[{slot}]: the seat's depth 04 clears the party-bank bit"
        );
        let want = FieldClipPlayer::from_record(&scene_anm, usize::from(clip) - 1)
            .map(|p| (p.frame_count(), p.bone_count()))
            .expect("scene flying clip decodes");
        assert_eq!(
            bound.get(&slot).copied(),
            Some(want),
            "P1[{slot}]: the seat binds scene clip {clip} (the flying pose), not the party idle"
        );
    }
}
