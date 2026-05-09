# Navmesh / per-scene region table

A 24-byte stride record table loaded into RAM at `0x80108EA4..0x801095xx` during scene loads. Carries per-scene NPC region / event-trigger / nav-region records — a candidate for the navmesh-class data the field VM and motion VM consult while pathing actors.

## How this was located

Diffing two area-load save states against each other (`mc1 = post-load early frame, scene = map01` vs. `mc3 = late frame, scene = suimon`) over the configured `navmesh_candidate` window `0x80100000..0x80120000` from [`scripts/mednafen/scenarios.toml`](../tooling/mednafen-automation.md) surfaces a 1700-byte cluster at `0x80108EA4..0x80109550` where the two scenes' contents differ. The first 0x40 bytes are byte-identical between the two saves, suggesting a small bank of shared records ahead of the per-scene records.

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

**Inferred — pending consumer trace.** The layout above is a structural inference from a single mednafen save-state diff pair. The consumer function (the field-VM op or motion-VM helper that reads this table) has not yet been pinned down — every candidate caller is in an already-captured overlay, but the read-side query against `0x80108EA4` requires a writer-search pass through `overlay_field_battle_intro.bin` / the field-overlay dump.

## Files referencing this format

- The field VM's actor-spawn family (script-VM ops `0x40`/`0x4F`) writes per-NPC records into a similarly-shaped table; the navmesh records may share storage.
- The motion VM (`FUN_8003774C`, per-actor pursue/patrol) likely consults this table when resolving "pursue target" coordinates.
