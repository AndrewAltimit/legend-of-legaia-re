#!/usr/bin/env python3
"""Collapse a glb's multi-UV-set materials onto TEXCOORD_0 for glTFast.

Unity's glTFast (the VRChat worlds importer) only supports UV sets 0/1 in
its shaders; a material sampling ``texCoord`` 2 or 3 logs the ``UVMulti``
error and the whole scripted import is marked failed - while web viewers,
which support arbitrary sets, show the same file fine. Blender bakes
against a late UV layer produce exactly this shape.

When every material samples at most ONE UV set (the common bake case, and
verified here), the fix is lossless: for each mesh primitive, point
``TEXCOORD_0`` at the accessor of the set its material actually samples,
drop the other UV attributes, and clear every ``texCoord`` index. Nothing
is resampled - the same coordinates just move to the stream slot glTFast
reads.

    python fix-glb-uvsets.py in.glb out.glb

Refuses (with a listing) if any material samples two different UV sets -
that needs a re-export, not a rewire.
"""

import json
import struct
import sys


def texture_infos(material):
    """Yield every textureInfo dict in a material (walks nested dicts)."""
    stack = [material]
    while stack:
        node = stack.pop()
        for key, value in node.items():
            if isinstance(value, dict):
                if isinstance(value.get("index"), int) and key.endswith("Texture"):
                    yield value
                stack.append(value)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]
    raw = open(src, "rb").read()
    magic, version, _total = struct.unpack_from("<III", raw, 0)
    if magic != 0x46546C67:
        sys.exit("not a glb: " + src)
    json_len, json_type = struct.unpack_from("<II", raw, 12)
    if json_type != 0x4E4F534A:
        sys.exit("first chunk is not JSON")
    gltf = json.loads(raw[20 : 20 + json_len])
    rest = raw[20 + json_len :]  # BIN chunk (and any others), byte-identical

    # Which single UV set does each material sample?
    mat_set = {}
    mixed = []
    for mi, mat in enumerate(gltf.get("materials", [])):
        sets = sorted({ti.get("texCoord", 0) for ti in texture_infos(mat)})
        if len(sets) > 1:
            mixed.append((mi, mat.get("name"), sets))
        mat_set[mi] = sets[0] if sets else 0
    if mixed:
        for mi, name, sets in mixed:
            print(f"material {mi} {name!r} samples UV sets {sets}")
        sys.exit("materials sample multiple UV sets - re-export instead")

    rewired = 0
    for mesh in gltf.get("meshes", []):
        for prim in mesh.get("primitives", []):
            attrs = prim.get("attributes", {})
            used = mat_set.get(prim.get("material"), 0)
            if used > 0:
                key = f"TEXCOORD_{used}"
                if key not in attrs:
                    sys.exit(f"mesh {mesh.get('name')!r} lacks {key}")
                attrs["TEXCOORD_0"] = attrs[key]
                rewired += 1
            for key in [k for k in attrs if k.startswith("TEXCOORD_") and k != "TEXCOORD_0"]:
                del attrs[key]
    for mat in gltf.get("materials", []):
        for ti in texture_infos(mat):
            ti.pop("texCoord", None)

    body = json.dumps(gltf, separators=(",", ":")).encode()
    body += b" " * (-len(body) % 4)
    out = struct.pack("<III", magic, version, 12 + 8 + len(body) + len(rest))
    out += struct.pack("<II", len(body), json_type) + body + rest
    open(dst, "wb").write(out)
    print(f"rewired {rewired} primitives onto TEXCOORD_0 -> {dst}")


if __name__ == "__main__":
    main()
