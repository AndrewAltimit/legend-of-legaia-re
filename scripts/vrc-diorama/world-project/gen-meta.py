#!/usr/bin/env python3
"""Generate Unity .meta sidecars for the LegaiaDiorama assets.

Unity creates a .meta (with a random GUID) for any asset lacking one on import.
Pre-generating them with DETERMINISTIC GUIDs (md5 of the project-relative path)
makes the drop-in stable: the same scripts get the same GUIDs on every machine,
so a prefab/scene can reference them reproducibly and re-imports don't churn.

Create-if-missing: never clobbers an existing .meta (so a GUID, once minted,
is stable even after codegen rewrites Registers.cs).

Usage:  scripts/vrc-diorama/world-project/gen-meta.py
"""

import hashlib
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
ASSETS = HERE / "Assets"


def guid_for(rel: str) -> str:
    # 32 lowercase hex chars = a valid Unity GUID.
    return hashlib.md5(("legaia-diorama:" + rel).encode()).hexdigest()


SCRIPT_META = """fileFormatVersion: 2
guid: {guid}
MonoImporter:
  externalObjects: {{}}
  serializedVersion: 2
  defaultReferences: []
  executionOrder: 0
  icon: {{instanceID: 0}}
  userData:
  assetBundleName:
  assetBundleVariant:
"""

FOLDER_META = """fileFormatVersion: 2
guid: {guid}
folderAsset: yes
DefaultImporter:
  externalObjects: {{}}
  userData:
  assetBundleName:
  assetBundleVariant:
"""


def rel(p: pathlib.Path) -> str:
    # Path relative to the Unity project root (the dir holding Assets/).
    return p.relative_to(HERE).as_posix()


def write_meta(target: pathlib.Path, body: str):
    meta = target.with_name(target.name + ".meta")
    if meta.exists():
        print(f"[gen-meta] keep   {rel(meta)}")
        return
    meta.write_text(body.format(guid=guid_for(rel(target))))
    print(f"[gen-meta] write  {rel(meta)}")


def main():
    # Folders (every dir under Assets/, plus Assets-relative subdirs) get a meta.
    for d in sorted(p for p in ASSETS.rglob("*") if p.is_dir()):
        write_meta(d, FOLDER_META)
    # .cs scripts get a MonoImporter meta.
    for cs in sorted(ASSETS.rglob("*.cs")):
        write_meta(cs, SCRIPT_META)


if __name__ == "__main__":
    main()
