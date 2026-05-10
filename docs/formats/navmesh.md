# Navmesh / per-scene region table

A 24-byte stride record table loaded into RAM at `0x80108EA4..0x801095xx` during scene loads. Carries per-scene NPC region / event-trigger / nav-region records - a candidate for the navmesh-class data the field VM and motion VM consult while pathing actors.

## How this was located

Diffing two area-load save states against each other (one taken in scene `map01`, one in scene `suimon`) over the configured `navmesh_candidate` window `0x80100000..0x80120000` from [`scripts/mednafen/scenarios.toml`](../tooling/mednafen-automation.md) surfaces a 1700-byte cluster at `0x80108EA4..0x80109550` where the two scenes' contents differ. The first 0x40 bytes are byte-identical between the two saves, suggesting a small bank of shared records ahead of the per-scene records.

## Layout

24-byte fixed stride. The leading byte is a sequential record id (records 0..N − 1); the second byte is a sub-id / kind flag.

```text
+0x00  u8   id          ; sequential 0, 1, 2, ...
+0x01  u8   kind         ; 0x00 for the leading shared bank, 0x01 for per-scene records
+0x02  i16  field_a      ; signed coordinate / size value (X?)
+0x04  i16  field_b      ; signed coordinate / size value (Y?)
+0x06  i16  field_c      ; signed coordinate / size value (Z?)
+0x08  i16  field_d      ;
+0x0A  i16  field_e      ;
+0x0C  i16  field_f      ;
+0x0E  i16  field_g      ;
+0x10  u32  tag          ; 4-byte ASCII tag (e.g. "ZZZ\0", "EEE\0", "XXX\0", "(((\0", "333T")
+0x14  u16  sub_a        ;
+0x16  u16  sub_b        ;
```

Records observed in the leading bank carry small ASCII tags: `(((\0` (`0x00282828`), `333T` (`0x54333333`), `ZZZ\0` (`0x005A5A5A`), `EEE\0` (`0x00454545`), `XXX\0` (`0x00585858`), `...\0` (`0x002E2E2E`). The repeating-3-character tags suggest debug names attached to template records used by the scene pre-fill / world-map renderer.

The per-scene records (`kind == 0x01`) follow and carry distinct tags whose shape varies by scene.

## Confidence

**Inferred - consumer not in any captured dump.** The layout above is a structural inference from a single mednafen save-state diff pair. A grep for any `LUI` + `ORI` pair targeting the `0x80108EA4..0x80109550` window across every `ghidra/scripts/funcs/*.txt` returns zero hits, meaning every consumer of this table lives in an overlay slice that hasn't been captured yet (the table address is in the `0x801XXXXX` data window that maps to scene-resident overlays).

The most likely consumer is the field-VM actor-spawn family (script-VM ops `0x40` / `0x4F`) or the motion VM (`FUN_8003774C`, per-actor pursue / patrol) - both already-captured but neither reads from `0x80108EA4` in their static body. The reads must therefore go through a pointer that gets populated at scene load time, in code that lives in one of the still-uncaptured `town01` / `field` / `world_map` overlays.

## Files referencing this format

- Field VM's actor-spawn family writes per-NPC records into a similarly-shaped table (same 24-byte stride, same leading id / kind bytes).
- Motion VM consults a per-actor pursue / patrol coordinate set with similar shape.

Both candidates point at the same conclusion: the navmesh table is consumed via a pointer the scene-load overlay sets up. The next step is a `mednafen-state diff` over the pointer storage range (`0x801C0000..0x801E0000`) across the two area-load saves to surface which RAM cell holds the table base - that's where the writer-search should focus once the cell is known.
