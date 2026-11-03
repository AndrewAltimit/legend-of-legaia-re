// The export's frame in, the kit's frame out.
//
// A current export (conventions.world_frame == "psx_rx180") is right-handed:
// a PSX point (x, y, z) is (x, -y, -z) in the world glb, in an NPC or prop
// file under its root node, and in every manifest number. The kit builds in
// its own older frame, which settings files, snapshots and every pass are
// written against: Z of the manifest's numbers negated, a -1 Z on each glb,
// and a -1 X on the root (the header of LegaiaWorldBuilder.cs).
//
// So one conversion runs at the door, in two parts:
//
// - Load() rewrites a current manifest's numbers: positions and facings have
//   Z negated; a rotation is conjugated by that same Z mirror, so a
//   quaternion's x and y change sign and a yaw turns the other way.
// - WorldToKitFrame() does the same to the world glb's node transforms, and
//   gives each mesh node the -1 Z the NPC and prop instances carry.
//
// After that nothing downstream can tell which export it was given, which
// is the point: a town built from either stands in the same place, with the
// same object_transforms, spawn and snapshot values. An older manifest has no
// world_frame and passes through untouched.

using System.Collections.Generic;
using System.IO;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaManifestFrame
    {
        const string ROTATION_FRAME = "psx_rx180";
        const string CONVERTED = "world_frame_converted_to_kit";

        /// Parse a scene manifest and bring its numbers into the kit's frame.
        internal static object Load(string path)
        {
            object m = MiniJson.Parse(File.ReadAllText(path));
            ToKitFrame(m);
            return m;
        }

        /// True for an export whose world glb and manifest are the right-handed
        /// frame (see the header). False for an older, mirrored export.
        internal static bool IsRotationFrame(object manifest)
        {
            return MiniJson.AsStr(MiniJson.Get(
                MiniJson.Get(manifest, "conventions"), "world_frame")) == ROTATION_FRAME;
        }

        static void ToKitFrame(object m)
        {
            var conventions = MiniJson.AsObj(MiniJson.Get(m, "conventions"));
            if (!IsRotationFrame(m) || conventions == null || conventions.ContainsKey(CONVERTED))
                return;
            conventions[CONVERTED] = true;

            NegateZ(MiniJson.Get(MiniJson.Get(m, "spawn"), "position"));
            foreach (object n in Items(m, "npcs"))
                NegateZ(MiniJson.Get(n, "position"));
            foreach (object d in Items(m, "doors"))
                NegateZ(MiniJson.Get(d, "position"));
            foreach (object p in Items(m, "animated_props"))
                foreach (object inst in Items(p, "instances"))
                {
                    NegateZ(MiniJson.Get(inst, "position"));
                    ConjugateRotation(inst);
                }
            foreach (object t in Items(m, "teleports"))
            {
                NegateZ(MiniJson.Get(MiniJson.Get(t, "trigger"), "position"));
                NegateZ(MiniJson.Get(MiniJson.Get(t, "destination"), "position"));
                NegateXzSecond(MiniJson.Get(t, "facing_dir"));
            }
            foreach (object p in Items(m, "scene_portals"))
            {
                NegateZ(MiniJson.Get(MiniJson.Get(p, "trigger"), "position"));
                NegateXzSecond(MiniJson.Get(p, "entry_xz"));
                NegateXzSecond(MiniJson.Get(p, "facing_dir"));
                NegateXzSecond(MiniJson.Get(MiniJson.Get(p, "conditional"), "entry_xz"));
            }
        }

        /// The world glb's half of the conversion. Every node's translation and
        /// rotation take the same Z mirror as the manifest's numbers, and each
        /// mesh node a -1 Z on its scale, which turns the glb's vertices back
        /// into what the transforms above them now expect.
        internal static void WorldToKitFrame(Transform world)
        {
            var nodes = new List<Transform>();
            foreach (Transform t in world.GetComponentsInChildren<Transform>(true))
                if (t != world)
                    nodes.Add(t);
            foreach (Transform t in nodes)
            {
                Vector3 p = t.localPosition;
                t.localPosition = new Vector3(p.x, p.y, -p.z);
                Quaternion q = t.localRotation;
                t.localRotation = new Quaternion(-q.x, -q.y, q.z, q.w);
                bool hasMesh = t.GetComponent<MeshFilter>() != null ||
                               t.GetComponent<SkinnedMeshRenderer>() != null;
                if (hasMesh)
                {
                    Vector3 s = t.localScale;
                    t.localScale = new Vector3(s.x, s.y, -s.z);
                }
            }
        }

        static IEnumerable<object> Items(object parent, string key)
        {
            return MiniJson.AsList(MiniJson.Get(parent, key)) ?? new List<object>();
        }

        static void NegateZ(object vec3)
        {
            var v = MiniJson.AsList(vec3);
            if (v != null && v.Count >= 3)
                v[2] = -MiniJson.AsNum(v[2]);
        }

        /// A `[x, z]` pair: entry points and facing directions.
        static void NegateXzSecond(object pair)
        {
            var v = MiniJson.AsList(pair);
            if (v != null && v.Count >= 2)
                v[1] = -MiniJson.AsNum(v[1]);
        }

        static void ConjugateRotation(object inst)
        {
            var obj = MiniJson.AsObj(inst);
            var q = MiniJson.AsList(MiniJson.Get(inst, "rotation_xyzw"));
            if (q != null && q.Count == 4)
            {
                q[0] = -MiniJson.AsNum(q[0]);
                q[1] = -MiniJson.AsNum(q[1]);
            }
            if (obj != null && obj.ContainsKey("rot_y_radians"))
                obj["rot_y_radians"] = -MiniJson.AsNum(obj["rot_y_radians"]);
        }
    }
}
