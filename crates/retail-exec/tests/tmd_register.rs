//! The TMD registrar `FUN_800268DC` off the disc, over every Legaia TMD the
//! raw PROT entries carry, against `legaia_tmd::parse`: where each object's
//! vertex, normal and primitive pools land once the registrar has made the
//! offsets absolute (each `top & !3` past the header), the vertices and
//! normals read back from there, the flags word it raises, and a registered
//! blob left alone on a second call.

use crate::common::disc;
use legaia_container_formats::tmd_scan::scan_buffer;
use psx_retail_exec::Stop;

const REGISTER: u32 = 0x8002_68DC;
const TMD: u32 = 0x8010_0000;

#[test]
fn the_registrar_resolves_what_parse_resolves() {
    differential!(scus(REGISTER));
    let (Some(mut cpu), Some(disc)) = (crate::common::scus_cpu(), disc()) else {
        return;
    };
    let (mut tmds, mut objects) = (0usize, 0usize);
    for entry in 0..1233 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for hit in scan_buffer(&buf).into_iter().take(64) {
            let bytes = &buf[hit.offset..hit.offset + hit.byte_len];
            if bytes.len() > 0xF_0000 {
                continue;
            }
            let Ok(port) = legaia_tmd::parse(bytes) else {
                continue;
            };
            cpu.mem.load(TMD, bytes);
            assert_eq!(cpu.call(REGISTER, &[TMD]), Stop::Returned);
            let ctx = format!("entry {entry} offset {:#x}", hit.offset);
            assert_eq!(cpu.mem.rd32(TMD + 4), 1, "flags, {ctx}");
            for (k, o) in port.objects.iter().enumerate() {
                let at = TMD + 12 + 0x1C * k as u32;
                let vert = cpu.mem.rd32(at);
                let normal = cpu.mem.rd32(at + 8);
                let prim = cpu.mem.rd32(at + 0x10);
                assert_eq!(
                    prim - TMD,
                    o.primitives_byte_offset as u32,
                    "object {k} primitives, {ctx}"
                );
                let read = |cpu: &mut psx_retail_exec::Cpu, p: u32, n: usize| -> Vec<[i16; 4]> {
                    (0..n)
                        .map(|v| {
                            std::array::from_fn(|c| {
                                cpu.mem.rd16(p + 8 * v as u32 + 2 * c as u32) as i16
                            })
                        })
                        .collect()
                };
                let verts: Vec<[i16; 4]> =
                    o.vertices.iter().map(|v| [v.x, v.y, v.z, v._pad]).collect();
                assert_eq!(
                    read(&mut cpu, vert, verts.len()),
                    verts,
                    "object {k} vertices, {ctx}"
                );
                let normals: Vec<[i16; 4]> =
                    o.normals.iter().map(|v| [v.x, v.y, v.z, v._pad]).collect();
                assert_eq!(
                    read(&mut cpu, normal, normals.len()),
                    normals,
                    "object {k} normals, {ctx}"
                );
                objects += 1;
            }
            // A second call sees `flags == 1` and leaves the pointers.
            let table = cpu.mem.read(TMD, 12 + 0x1C * port.objects.len());
            assert_eq!(cpu.call(REGISTER, &[TMD]), Stop::Returned);
            assert_eq!(
                cpu.mem.read(TMD, table.len()),
                table,
                "re-registration, {ctx}"
            );
            tmds += 1;
        }
    }
    assert!(
        tmds > 500 && objects > 1500,
        "tmds {tmds} objects {objects}"
    );
}
