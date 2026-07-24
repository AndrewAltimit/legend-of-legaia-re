//! One-off reader for the Gaza 2 parked savestate: dump each monster seat's
//! action-tag table (record +0x4C, count +0x4A) to test whether tag 0x20
//! (the state-0x14 walk lookup) exists. Usage:
//!   cargo run -p legaia-pcsxr --example gaza2_walk_tag -- <path.sstate>

use legaia_pcsxr::SaveState;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gaza2_walk_tag <path.sstate>");
    let st = SaveState::from_path(std::path::Path::new(&path)).expect("load sstate");
    println!("scene={} mode=0x{:02X}", st.scene_name(), st.game_mode());

    let ctx = st.u32_at(0x8007BD24);
    println!(
        "ctx=0x{:08X} ctx7=0x{:02X} acting=ctx+0x13={} c6d4={} c6d8={}",
        ctx,
        st.u8_at(ctx + 7),
        st.u8_at(ctx + 0x13),
        st.u16_at(ctx + 0x6D4),
        st.i16_at(ctx + 0x6D8) as i32,
    );

    // Actor pointer table (all 8 seats) for cross-reference.
    for seat in 0..8u32 {
        let a = st.u32_at(0x801C9370 + seat * 4);
        if !(0x8000_0000..0x8020_0000).contains(&a) {
            continue;
        }
        println!(
            "seat {}: actor=0x{:08X} hp={}/{} target(+0x1DD)={} anim(+0x1DA)=0x{:02X} anim_now(+0x1D9)=0x{:02X}",
            seat,
            a,
            st.u16_at(a + 0x14C),
            st.u16_at(a + 0x14E),
            st.u8_at(a + 0x1DD),
            st.u8_at(a + 0x1DA),
            st.u8_at(a + 0x1D9),
        );
    }

    // Per-monster records: DAT_801c9348[seat-3] for seats 3..7.
    for slot in 0..5u32 {
        let rec = st.u32_at(0x801C9348 + slot * 4);
        if !(0x8000_0000..0x8020_0000).contains(&rec) {
            continue;
        }
        let count = st.u8_at(rec + 0x4A);
        print!(
            "seat {}: rec=0x{:08X} action_count={} tags=[",
            slot + 3,
            rec,
            count
        );
        let mut has_walk = false;
        for i in 0..count as u32 {
            let action = st.u32_at(rec + 0x4C + i * 4);
            let tag = if (0x8000_0000..0x8020_0000).contains(&action) {
                st.u8_at(action)
            } else {
                0xEE
            };
            if tag == 0x20 {
                has_walk = true;
            }
            print!("{}0x{:02X}", if i == 0 { "" } else { " " }, tag);
        }
        println!(
            "]  walk_tag_0x20={}",
            if has_walk { "PRESENT" } else { "ABSENT" }
        );
    }
}
