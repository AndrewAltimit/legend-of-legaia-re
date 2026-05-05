# Field-pack format

Magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. A small number of PROT entries match. Detector + dispatch: `crates/asset/src/field_pack.rs`.

The preamble → slot mapping is not yet pinned down — likely reconstructed at runtime from offset hints in the schema. Use the detector for classification today; full per-slot interpretation is pending a runtime trace.
