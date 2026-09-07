// Generated art for the living town's speech bubbles: fourteen pictogram
// textures drawn from scratch, one material each, and the unit quad the
// bubble renders on. Six reactions ("...", "!", "?", heart, music note,
// laugh), the daytime pair (a raised hand a villager waves when it passes
// a neighbour, the sweat drops it shows while busy at an errand stop), and
// six conversation TOPICS (fish, house, sun, sleep, food, storm cloud) the
// director opens a chat with. Pictograms are the whole language - no
// dialog text - the way The Sims lets a picture stand for the sentence.
//
// Everything here is procedural - no game data, no imported sprite - so the
// bubbles ship with the kit like the card faces and the slot marquee do.
// The textures are point-filtered and cut out on alpha, which keeps them
// in the exports' PSX register rather than reading as a modern UI overlay.
//
// The material shader is the built-in `Sprites/Default`: it is two-sided
// (Cull Off) and premultiplied-alpha blended, which is what a billboarded
// quad under the builder's MIRRORED root needs - a single-sided shader
// would render nothing from half the angles the mirror produces, and the
// bubble behaviour's own 180-degree measurement cannot fix a culled face.
//
// One atlas is written per scene under
// Assets/LegaiaGenerated/<scene>/livingtown/, so re-running the pass
// refreshes instead of stacking.

using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaBubbleArt
    {
        public const int ICONS = 14;
        const int SIZE = 96;

        public static readonly string[] ICON_NAMES =
            { "talk", "excl", "question", "heart", "music", "laugh",
              "wave", "work",
              "fish", "house", "sun", "sleep", "food", "storm" };

        /// Index of the raised hand (LegaiaTownDirector mirrors it as a
        /// constant, since Udon has no reach into an editor class).
        public const int WAVE = 6;

        /// Index of the work drops.
        public const int WORK = 7;

        /// The topic band, in ICON_NAMES order (the director mirrors the
        /// first index and the count as constants).
        public const int FISH = 8;
        public const int HOUSE = 9;
        public const int SUN = 10;
        public const int SLEEP = 11;
        public const int FOOD = 12;
        public const int STORM = 13;

        /// One material per icon, created (or refreshed) under `genDir`.
        public static Material[] IconMaterials(string genDir)
        {
            Directory.CreateDirectory(genDir);
            var shader = Shader.Find("Sprites/Default");
            if (shader == null)
                shader = Shader.Find("Unlit/Transparent");
            var mats = new Material[ICONS];
            for (int i = 0; i < ICONS; i++)
            {
                string texPath = genDir + "/bubble_" + ICON_NAMES[i] + ".png";
                var tex = Draw(i);
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath, ImportAssetOptions.ForceUpdate);
                var imp = AssetImporter.GetAtPath(texPath) as TextureImporter;
                if (imp != null)
                {
                    imp.textureType = TextureImporterType.Default;
                    imp.alphaIsTransparency = true;
                    imp.filterMode = FilterMode.Point;
                    imp.wrapMode = TextureWrapMode.Clamp;
                    imp.mipmapEnabled = false;
                    imp.SaveAndReimport();
                }
                var asset = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);

                string matPath = genDir + "/bubble_" + ICON_NAMES[i] + ".mat";
                var mat = AssetDatabase.LoadAssetAtPath<Material>(matPath);
                if (mat == null)
                {
                    mat = new Material(shader);
                    AssetDatabase.CreateAsset(mat, matPath);
                }
                mat.shader = shader;
                mat.mainTexture = asset;
                EditorUtility.SetDirty(mat);
                mats[i] = mat;
            }
            return mats;
        }

        /// A unit quad in the XY plane whose FRONT is +Z (both faces render -
        /// the material is Cull Off - so the winding is not load-bearing).
        public static Mesh QuadMesh(string genDir)
        {
            Directory.CreateDirectory(genDir);
            string path = genDir + "/bubble_quad.asset";
            var mesh = AssetDatabase.LoadAssetAtPath<Mesh>(path);
            if (mesh != null)
                return mesh;
            mesh = new Mesh();
            mesh.name = "bubble_quad";
            mesh.vertices = new[]
            {
                new Vector3(-0.5f, -0.5f, 0f), new Vector3(0.5f, -0.5f, 0f),
                new Vector3(-0.5f, 0.5f, 0f), new Vector3(0.5f, 0.5f, 0f),
            };
            mesh.uv = new[]
            {
                new Vector2(0f, 0f), new Vector2(1f, 0f),
                new Vector2(0f, 1f), new Vector2(1f, 1f),
            };
            mesh.normals = new[]
            {
                Vector3.forward, Vector3.forward, Vector3.forward, Vector3.forward,
            };
            mesh.triangles = new[] { 0, 2, 1, 2, 3, 1 };
            mesh.RecalculateBounds();
            AssetDatabase.CreateAsset(mesh, path);
            return mesh;
        }

        // --- Drawing ---------------------------------------------------------

        static Color32[] buf;

        static Texture2D Draw(int icon)
        {
            buf = new Color32[SIZE * SIZE];
            for (int i = 0; i < buf.Length; i++)
                buf[i] = new Color32(0, 0, 0, 0);

            // Balloon: a rounded rect with a tail on the bottom-left,
            // cream fill with a dark rim (the retail dialog box's palette).
            var fill = new Color32(248, 244, 226, 255);
            var rim = new Color32(38, 30, 26, 255);
            Balloon(rim, 3.0f);
            Balloon(fill, 0f);

            var ink = new Color32(38, 30, 26, 255);
            if (icon == 0)
            {
                Disc(28f, 56f, 6f, ink);
                Disc(48f, 56f, 6f, ink);
                Disc(68f, 56f, 6f, ink);
            }
            else if (icon == 1)
            {
                Bar(48f, 46f, 84f, 7f, ink);
                Disc(48f, 36f, 6f, ink);
            }
            else if (icon == 2)
            {
                Arc(48f, 70f, 13f, 6f, 200f, -240f, ink);
                Stroke(58f, 62f, 48f, 52f, 6f, ink);
                Bar(48f, 44f, 52f, 6f, ink);
                Disc(48f, 34f, 6f, ink);
            }
            else if (icon == 3)
            {
                Heart(48f, 58f, 26f, new Color32(214, 62, 88, 255));
            }
            else if (icon == 4)
            {
                Ellipse(38f, 44f, 11f, 8f, ink);
                Bar(48f, 44f, 82f, 5f, ink);
                Tri(48f, 82f, 68f, 74f, 48f, 68f, ink);
            }
            else if (icon == 6)
            {
                // A raised open hand: palm plus four fingers and a thumb.
                Bar(48f, 40f, 62f, 26f, ink);        // palm
                Disc(48f, 40f, 13f, ink);            // heel of the hand
                Bar(34f, 60f, 74f, 7f, ink);         // fingers
                Bar(44f, 60f, 78f, 7f, ink);
                Bar(54f, 60f, 78f, 7f, ink);
                Bar(64f, 60f, 72f, 7f, ink);
                Stroke(36f, 46f, 26f, 58f, 8f, ink); // thumb
            }
            else if (icon == 7)
            {
                // Effort: three drops, the way a hand-drawn sprite sweats.
                Drop(34f, 62f, 9f, ink);
                Drop(50f, 70f, 11f, ink);
                Drop(66f, 60f, 8f, ink);
            }
            else if (icon == FISH)
            {
                // A fish swimming left: body, forked tail, one eye.
                var sea = new Color32(64, 112, 196, 255);
                Ellipse(44f, 58f, 19f, 10f, sea);
                Tri(60f, 58f, 76f, 70f, 76f, 46f, sea);
                Disc(34f, 60f, 3f, fill);
                Disc(34f, 60f, 1.5f, ink);
            }
            else if (icon == HOUSE)
            {
                // A hut: walls, a wide roof, a door.
                Bar(48f, 34f, 58f, 34f, ink);
                Tri(24f, 56f, 72f, 56f, 48f, 82f, new Color32(168, 74, 52, 255));
                Bar(48f, 34f, 50f, 9f, fill);
            }
            else if (icon == SUN)
            {
                // A sun with eight rays.
                var gold = new Color32(236, 184, 48, 255);
                for (int i = 0; i < 8; i++)
                {
                    float a = i * Mathf.PI / 4f;
                    Stroke(48f + Mathf.Cos(a) * 17f, 58f + Mathf.Sin(a) * 17f,
                        48f + Mathf.Cos(a) * 25f, 58f + Mathf.Sin(a) * 25f, 4f, gold);
                }
                Disc(48f, 58f, 13f, gold);
            }
            else if (icon == SLEEP)
            {
                // Three Zs, growing toward the top right.
                Zed(30f, 44f, 10f, ink);
                Zed(46f, 58f, 13f, ink);
                Zed(64f, 72f, 16f, ink);
            }
            else if (icon == FOOD)
            {
                // A loaf with three scores across the crust.
                var crust = new Color32(204, 142, 70, 255);
                Ellipse(48f, 56f, 21f, 12f, ink);
                Ellipse(48f, 56f, 18f, 9f, crust);
                Stroke(38f, 62f, 44f, 54f, 3f, ink);
                Stroke(46f, 63f, 52f, 55f, 3f, ink);
                Stroke(54f, 62f, 60f, 54f, 3f, ink);
            }
            else if (icon == STORM)
            {
                // A grey cloud over a yellow bolt: grumbling about the weather.
                var grey = new Color32(112, 116, 128, 255);
                Disc(37f, 66f, 9f, grey);
                Disc(50f, 71f, 11f, grey);
                Disc(63f, 66f, 9f, grey);
                Ellipse(50f, 62f, 23f, 7f, grey);
                var bolt = new Color32(236, 184, 48, 255);
                Stroke(52f, 56f, 45f, 46f, 4f, bolt);
                Stroke(45f, 46f, 53f, 46f, 4f, bolt);
                Stroke(53f, 46f, 45f, 34f, 4f, bolt);
            }
            else
            {
                // "^ ^" eyes over a wide grin.
                Stroke(28f, 62f, 36f, 72f, 5f, ink);
                Stroke(36f, 72f, 44f, 62f, 5f, ink);
                Stroke(52f, 62f, 60f, 72f, 5f, ink);
                Stroke(60f, 72f, 68f, 62f, 5f, ink);
                Arc(48f, 54f, 15f, 5f, 190f, 160f, ink);
            }

            var tex = new Texture2D(SIZE, SIZE, TextureFormat.RGBA32, false);
            tex.SetPixels32(buf);
            tex.Apply();
            return tex;
        }

        static void Px(int x, int y, Color32 c)
        {
            if (x < 0 || y < 0 || x >= SIZE || y >= SIZE)
                return;
            if (c.a == 0)
                return;
            buf[y * SIZE + x] = c;
        }

        // Rounded-rect balloon plus its tail, grown by `grow` pixels (the
        // outline pass draws the same shape fatter underneath the fill).
        static void Balloon(Color32 c, float grow)
        {
            float x0 = 8f - grow, x1 = 88f + grow;
            float y0 = 26f - grow, y1 = 90f + grow;
            float r = 14f + grow;
            for (int y = 0; y < SIZE; y++)
                for (int x = 0; x < SIZE; x++)
                {
                    float fx = x + 0.5f, fy = y + 0.5f;
                    bool inside = false;
                    if (fx >= x0 + r && fx <= x1 - r && fy >= y0 && fy <= y1)
                        inside = true;
                    else if (fy >= y0 + r && fy <= y1 - r && fx >= x0 && fx <= x1)
                        inside = true;
                    else
                    {
                        float cx = fx < x0 + r ? x0 + r : x1 - r;
                        float cy = fy < y0 + r ? y0 + r : y1 - r;
                        inside = (fx - cx) * (fx - cx) + (fy - cy) * (fy - cy) <= r * r;
                    }
                    // Tail: a wedge from the balloon's bottom edge down to
                    // the NPC's head.
                    if (!inside && fy < y0 && fy > 6f - grow)
                    {
                        float t = (y0 - fy) / (y0 - 6f);
                        float half = (11f + grow) * (1f - t);
                        if (Mathf.Abs(fx - (34f - 8f * t)) <= half)
                            inside = true;
                    }
                    if (inside)
                        Px(x, y, c);
                }
        }

        static void Disc(float cx, float cy, float r, Color32 c)
        {
            for (int y = (int)(cy - r) - 1; y <= cy + r + 1; y++)
                for (int x = (int)(cx - r) - 1; x <= cx + r + 1; x++)
                {
                    float dx = x + 0.5f - cx, dy = y + 0.5f - cy;
                    if (dx * dx + dy * dy <= r * r)
                        Px(x, y, c);
                }
        }

        static void Ellipse(float cx, float cy, float rx, float ry, Color32 c)
        {
            for (int y = (int)(cy - ry) - 1; y <= cy + ry + 1; y++)
                for (int x = (int)(cx - rx) - 1; x <= cx + rx + 1; x++)
                {
                    float dx = (x + 0.5f - cx) / rx, dy = (y + 0.5f - cy) / ry;
                    if (dx * dx + dy * dy <= 1f)
                        Px(x, y, c);
                }
        }

        static void Bar(float cx, float y0, float y1, float w, Color32 c)
        {
            for (int y = (int)y0; y <= y1; y++)
                for (int x = (int)(cx - w * 0.5f); x <= cx + w * 0.5f; x++)
                    Px(x, y, c);
        }

        static void Stroke(float ax, float ay, float bx, float by, float w, Color32 c)
        {
            int steps = (int)(Mathf.Max(Mathf.Abs(bx - ax), Mathf.Abs(by - ay)) * 2f) + 1;
            for (int i = 0; i <= steps; i++)
            {
                float t = i / (float)steps;
                Disc(Mathf.Lerp(ax, bx, t), Mathf.Lerp(ay, by, t), w * 0.5f, c);
            }
        }

        static void Arc(float cx, float cy, float r, float w,
            float startDeg, float sweepDeg, Color32 c)
        {
            int steps = (int)Mathf.Abs(sweepDeg);
            for (int i = 0; i <= steps; i++)
            {
                float a = (startDeg + sweepDeg * i / steps) * Mathf.Deg2Rad;
                Disc(cx + Mathf.Cos(a) * r, cy + Mathf.Sin(a) * r, w * 0.5f, c);
            }
        }

        static void Tri(float ax, float ay, float bx, float by,
            float cx2, float cy2, Color32 c)
        {
            float minX = Mathf.Min(ax, Mathf.Min(bx, cx2));
            float maxX = Mathf.Max(ax, Mathf.Max(bx, cx2));
            float minY = Mathf.Min(ay, Mathf.Min(by, cy2));
            float maxY = Mathf.Max(ay, Mathf.Max(by, cy2));
            for (int y = (int)minY; y <= maxY; y++)
                for (int x = (int)minX; x <= maxX; x++)
                {
                    float px = x + 0.5f, py = y + 0.5f;
                    float d1 = Side(px, py, ax, ay, bx, by);
                    float d2 = Side(px, py, bx, by, cx2, cy2);
                    float d3 = Side(px, py, cx2, cy2, ax, ay);
                    bool neg = d1 < 0 || d2 < 0 || d3 < 0;
                    bool pos = d1 > 0 || d2 > 0 || d3 > 0;
                    if (!(neg && pos))
                        Px(x, y, c);
                }
        }

        static float Side(float px, float py, float ax, float ay, float bx, float by)
        {
            return (px - bx) * (ay - by) - (ax - bx) * (py - by);
        }

        // A letter Z `size` pixels across, centred on (cx, cy).
        static void Zed(float cx, float cy, float size, Color32 c)
        {
            float h = size / 2f, w = size < 12f ? 3f : 4f;
            Stroke(cx - h, cy + h, cx + h, cy + h, w, c);
            Stroke(cx + h, cy + h, cx - h, cy - h, w, c);
            Stroke(cx - h, cy - h, cx + h, cy - h, w, c);
        }

        // A teardrop: a disc with a point on top, `size` pixels tall.
        static void Drop(float cx, float cy, float size, Color32 c)
        {
            Disc(cx, cy - size * 0.25f, size * 0.55f, c);
            Tri(cx, cy + size * 0.9f,
                cx - size * 0.42f, cy - size * 0.1f,
                cx + size * 0.42f, cy - size * 0.1f, c);
        }

        // The classic implicit heart, scaled to `size` pixels across.
        static void Heart(float cx, float cy, float size, Color32 c)
        {
            for (int y = (int)(cy - size); y <= cy + size; y++)
                for (int x = (int)(cx - size); x <= cx + size; x++)
                {
                    float u = (x + 0.5f - cx) / (size * 0.62f);
                    float v = (y + 0.5f - cy) / (size * 0.62f);
                    float t = u * u + v * v - 1f;
                    if (t * t * t - u * u * v * v * v <= 0f)
                        Px(x, y, c);
                }
        }
    }
}
