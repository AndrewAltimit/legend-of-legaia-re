// The things villagers carry: a bucket, a broom, a bundle of firewood and
// a basket, built out of Unity primitives with generated flat materials -
// the same recipe the camp props use, so nothing here is game data and the
// items ship with the kit.
//
// One set is built per VILLAGER, parented under that villager's `carry`
// node (an empty at the measured hand point - see
// LegaiaLivingTown.BuildCarry), and every item starts INACTIVE:
// LegaiaNpcCarry enables exactly one while something is in hand. They are
// children of the NPC, so they follow it around for free and no per-frame
// "hold the prop at the hand" code exists anywhere.
//
// SIZE. Nothing here is in metres: every dimension is a fraction of the
// villager's own measured height, which is what keeps a bucket bucket-sized
// on a 0.7 m rig and on a 1.1 m one, at whatever scale the scene exported
// at. The local frame these are built in is the NPC's, so the instance's
// and the root's mirrors never enter the arithmetic - and a primitive
// cylinder or cube is symmetric under them anyway.
//
// The materials are shared per scene (one bucket material, not one per
// villager) so fifteen villagers add four materials, not sixty.

using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaCarryArt
    {
        internal const int ITEMS = 4;

        internal static readonly string[] ITEM_NAMES =
            { "bucket", "broom", "firewood", "basket" };

        /// Build one villager's set of items under `hand`, sized against a
        /// body height of `h` in that villager's LOCAL units. Returns the
        /// per-item root objects, all inactive, in ITEM_NAMES order.
        internal static GameObject[] Build(Transform hand, string genDir, float h)
        {
            Directory.CreateDirectory(genDir);
            Material wood = Mat(genDir, "carry_wood", new Color(0.42f, 0.30f, 0.19f));
            Material metal = Mat(genDir, "carry_metal", new Color(0.55f, 0.57f, 0.60f));
            Material straw = Mat(genDir, "carry_straw", new Color(0.76f, 0.64f, 0.36f));
            Material water = Mat(genDir, "carry_water", new Color(0.29f, 0.47f, 0.56f));

            var items = new GameObject[ITEMS];
            items[0] = Bucket(hand, h, metal, water);
            items[1] = Broom(hand, h, wood, straw);
            items[2] = Firewood(hand, h, wood);
            items[3] = Basket(hand, h, straw);
            for (int i = 0; i < ITEMS; i++)
                items[i].SetActive(false);
            return items;
        }

        // --- Items -------------------------------------------------------------

        // A pail hanging from the hand: a short barrel, a rim, a handle arc
        // over it, and a disc of water just below the rim.
        static GameObject Bucket(Transform hand, float h, Material metal, Material water)
        {
            var go = Root(hand, "item_bucket");
            float r = h * 0.075f, d = h * 0.10f;
            Prim(PrimitiveType.Cylinder, "body", go.transform,
                new Vector3(0f, -d, 0f), new Vector3(r * 2f, d, r * 2f), metal);
            Prim(PrimitiveType.Cylinder, "rim", go.transform,
                new Vector3(0f, -d * 0.06f, 0f),
                new Vector3(r * 2.2f, d * 0.09f, r * 2.2f), metal);
            Prim(PrimitiveType.Cylinder, "water", go.transform,
                new Vector3(0f, -d * 0.35f, 0f),
                new Vector3(r * 1.8f, d * 0.04f, r * 1.8f), water);
            // Handle: four short bars around a half circle, cheaper than a
            // torus and indistinguishable at villager scale.
            for (int i = 0; i < 4; i++)
            {
                float t0 = i / 4f, t1 = (i + 1) / 4f;
                Vector3 a = Arc(r * 1.05f, h * 0.055f, t0);
                Vector3 b = Arc(r * 1.05f, h * 0.055f, t1);
                Vector3 mid = (a + b) * 0.5f;
                var seg = Prim(PrimitiveType.Cube, "handle_" + i, go.transform,
                    mid, new Vector3(h * 0.012f, (b - a).magnitude, h * 0.012f), metal);
                seg.transform.localRotation =
                    Quaternion.FromToRotation(Vector3.up, (b - a).normalized);
            }
            return go;
        }

        static Vector3 Arc(float radius, float rise, float t)
        {
            float a = Mathf.PI * t;
            return new Vector3(-Mathf.Cos(a) * radius, Mathf.Sin(a) * rise, 0f);
        }

        // A besom: a long shaft with a splayed head of straw at the bottom.
        static GameObject Broom(Transform hand, float h, Material wood, Material straw)
        {
            var go = Root(hand, "item_broom");
            float len = h * 0.62f;
            Prim(PrimitiveType.Cylinder, "shaft", go.transform,
                new Vector3(0f, -len * 0.35f, 0f),
                new Vector3(h * 0.022f, len * 0.5f, h * 0.022f), wood);
            var head = Prim(PrimitiveType.Cylinder, "head", go.transform,
                new Vector3(0f, -len * 0.92f, 0f),
                new Vector3(h * 0.075f, len * 0.13f, h * 0.05f), straw);
            head.transform.localRotation = Quaternion.Euler(0f, 0f, 6f);
            return go;
        }

        // A bundle: five short logs held in a fan, with two bands round it.
        static GameObject Firewood(Transform hand, float h, Material wood)
        {
            var go = Root(hand, "item_firewood");
            float len = h * 0.24f;
            for (int i = 0; i < 5; i++)
            {
                float off = (i - 2) * h * 0.022f;
                var log = Prim(PrimitiveType.Cylinder, "log_" + i, go.transform,
                    new Vector3(off * 0.7f, -h * 0.03f, off * 0.5f),
                    new Vector3(h * 0.026f, len * 0.5f, h * 0.026f), wood);
                log.transform.localRotation =
                    Quaternion.Euler(90f, 0f, (i - 2) * 7f);
            }
            for (int i = 0; i < 2; i++)
                Prim(PrimitiveType.Cylinder, "band_" + i, go.transform,
                    new Vector3(0f, -h * 0.03f, (i == 0 ? -1f : 1f) * len * 0.28f),
                    new Vector3(h * 0.10f, h * 0.008f, h * 0.10f), wood)
                    .transform.localRotation = Quaternion.Euler(90f, 0f, 0f);
            return go;
        }

        // A shallow open basket carried at the hip.
        static GameObject Basket(Transform hand, float h, Material straw)
        {
            var go = Root(hand, "item_basket");
            float r = h * 0.095f;
            Prim(PrimitiveType.Cylinder, "body", go.transform,
                new Vector3(0f, -h * 0.06f, 0f),
                new Vector3(r * 2f, h * 0.055f, r * 1.5f), straw);
            Prim(PrimitiveType.Cylinder, "rim", go.transform,
                new Vector3(0f, -h * 0.006f, 0f),
                new Vector3(r * 2.15f, h * 0.008f, r * 1.62f), straw);
            return go;
        }

        // --- Helpers -----------------------------------------------------------

        static GameObject Root(Transform hand, string name)
        {
            var go = new GameObject(name);
            go.transform.SetParent(hand, false);
            return go;
        }

        static GameObject Prim(PrimitiveType type, string name, Transform parent,
            Vector3 localPos, Vector3 localScale, Material mat)
        {
            var go = GameObject.CreatePrimitive(type);
            go.name = name;
            // Carried props are cosmetic: a collider on one would push the
            // villager (and any player standing next to it) around.
            Object.DestroyImmediate(go.GetComponent<Collider>());
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            go.transform.localScale = localScale;
            var mr = go.GetComponent<MeshRenderer>();
            mr.sharedMaterial = mat;
            mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            mr.receiveShadows = false;
            return go;
        }

        static Material Mat(string genDir, string name, Color c)
        {
            string path = genDir + "/" + name + ".mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (m == null)
            {
                var shader = Shader.Find("Legacy Shaders/Diffuse")
                             ?? Shader.Find("Standard");
                m = new Material(shader);
                AssetDatabase.CreateAsset(m, path);
            }
            m.color = c;
            EditorUtility.SetDirty(m);
            return m;
        }
    }
}
