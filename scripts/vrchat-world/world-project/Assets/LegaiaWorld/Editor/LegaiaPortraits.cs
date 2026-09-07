// Villager portraits for the card table's seat panel: one head-and-
// shoulders render per NPC, saved as a PNG under the living-town
// generated folder and stored on the villager's brain (`portrait`), so a
// panel can show WHO is sitting in a chair with a picture instead of a
// name.
//
// HOW THE HEAD IS FOUND. Nothing here trusts an axis. The framing box is
// the villager's MEASURED renderer bounds (the rigs stand well under a
// metre at the kit's export scale, and no two are the same height), and
// the top 35% of that box is the portrait. The direction the camera looks
// FROM is the villager's visible front, read the same way the locomotion
// controller reads it at runtime: pick the largest mesh node whose
// rendered rest pose stands upright (the torso - limbs and heads rest
// tilted), then push the instance-frame +Z face axis through that node's
// full matrix as a TransformPoint difference, so the builder's scale
// mirrors and the importer's handedness conversion are both included. A
// TransformDirection would ignore scale and hand back the back of the
// head on every mirrored rig.
//
// ISOLATION. The villager is moved 3 km below the world for the length of
// one render and put back in a `finally` - cheaper and less fragile than
// cloning a rig that carries Udon behaviours, and it guarantees no wall,
// no neighbour and no ground is in the frame. The camera is orthographic
// so the framing is exact arithmetic rather than an FOV fit.
//
// HEADLESS. `-batchmode -nographics` has no graphics device at all, and
// Camera.Render into a RenderTexture is a hard failure there - so is any
// project whose GPU path throws for its own reasons. Rather than leaving
// the panel with no pictures (which would also make the card-game checks
// vacuous), the fallback draws the portrait on the CPU: a flat
// head-and-shoulders silhouette in a hue derived from the villager's own
// label, which is deterministic, needs no device, and still tells two
// seats apart at a glance. Which path ran is logged.
//
// Idempotent: the same villagers in the same order produce the same file
// names and the same pixels, and nothing is left in the scene.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace LegaiaWorld
{
    internal static class LegaiaPortraits
    {
        const int SIZE = 128;

        /// Render a portrait for every brain and set its `portrait` field.
        /// Returns how many were rendered.
        internal static int Apply(GameObject root, List<Component> brains,
            string sceneName, string genDir)
        {
            if (brains == null || brains.Count == 0)
                return 0;
            string dir = genDir + "/portraits";
            Directory.CreateDirectory(dir);

            bool gpu = CanRender();
            int rendered = 0, drawn = 0;
            for (int i = 0; i < brains.Count; i++)
            {
                Component brain = brains[i];
                if (brain == null)
                    continue;
                string token = Token(brain, i);
                string path = dir + "/" + token + ".png";

                Texture2D tex = null;
                if (gpu)
                {
                    try
                    {
                        tex = RenderHead(brain.transform);
                    }
                    catch (System.Exception e)
                    {
                        Debug.LogWarning("[Legaia] portraits: render failed on " +
                            token + " (" + e.Message + ") - falling back to the " +
                            "drawn silhouette for the rest of this pass.");
                        gpu = false;
                        tex = null;
                    }
                }
                if (tex != null)
                    rendered++;
                else
                {
                    tex = Silhouette(token);
                    drawn++;
                }

                File.WriteAllBytes(path, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(path, ImportAssetOptions.ForceUpdate);
                var imp = AssetImporter.GetAtPath(path) as TextureImporter;
                if (imp != null)
                {
                    imp.textureType = TextureImporterType.Default;
                    imp.wrapMode = TextureWrapMode.Clamp;
                    imp.filterMode = FilterMode.Bilinear;
                    imp.mipmapEnabled = false;
                    imp.npotScale = TextureImporterNPOTScale.None;
                    imp.maxTextureSize = SIZE;
                    imp.SaveAndReimport();
                }
                var asset = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
                LegaiaWorldBuilder.SetUdonField(brain, "portrait", asset);
                LegaiaWorldBuilder.SyncUdonProxy(brain);
            }
            Debug.Log("[Legaia] portraits: " + rendered + " rendered, " + drawn +
                " drawn on the CPU (no graphics device or the render path " +
                "refused) -> " + dir);
            return rendered + drawn;
        }

        static bool CanRender()
        {
            if (SystemInfo.graphicsDeviceType == GraphicsDeviceType.Null)
                return false;
            foreach (string a in System.Environment.GetCommandLineArgs())
                if (a == "-nographics")
                    return false;
            return true;
        }

        /// A file-safe, per-villager unique name: the brain's label when it
        /// has one, else the object name, plus its index.
        static string Token(Component brain, int index)
        {
            string label = null;
            var f = brain.GetType().GetField("label");
            if (f != null)
                label = f.GetValue(brain) as string;
            if (string.IsNullOrEmpty(label))
                label = brain.gameObject.name;
            var sb = new System.Text.StringBuilder();
            foreach (char c in label)
                sb.Append(char.IsLetterOrDigit(c) ? char.ToLowerInvariant(c) : '_');
            string s = sb.ToString();
            if (s.Length > 24)
                s = s.Substring(0, 24);
            return index.ToString("00") + "_" + s;
        }

        // --- the render path ------------------------------------------------------

        static Texture2D RenderHead(Transform npc)
        {
            var rends = npc.GetComponentsInChildren<Renderer>();
            if (rends.Length == 0)
                return null;

            Vector3 keep = npc.position;
            GameObject camGo = null, lightGo = null;
            RenderTexture rt = null;
            RenderTexture prev = RenderTexture.active;
            try
            {
                // Out of the world for one frame: no wall, no neighbour, no
                // ground can be in the frame.
                npc.position = keep + Vector3.down * 3000f;

                Bounds b = rends[0].bounds;
                for (int i = 1; i < rends.Length; i++)
                    b.Encapsulate(rends[i].bounds);
                if (b.size.y < 1e-4f)
                    return null;

                float headH = Mathf.Max(b.size.y * 0.35f, 0.02f);
                Vector3 centre = new Vector3(b.center.x, b.max.y - headH * 0.5f,
                    b.center.z);
                Vector3 front = VisualFront(npc);
                float dist = b.size.magnitude + 1f;

                camGo = new GameObject("~legaia-portrait-cam");
                camGo.hideFlags = HideFlags.HideAndDontSave;
                var cam = camGo.AddComponent<Camera>();
                cam.orthographic = true;
                cam.orthographicSize = headH * 0.62f;
                cam.clearFlags = CameraClearFlags.SolidColor;
                cam.backgroundColor = new Color(0.10f, 0.10f, 0.13f, 1f);
                cam.allowHDR = false;
                cam.allowMSAA = false;
                cam.nearClipPlane = 0.01f;
                cam.farClipPlane = dist * 2f + 2f;
                camGo.transform.position = centre + front * dist;
                camGo.transform.rotation = Quaternion.LookRotation(-front, Vector3.up);

                lightGo = new GameObject("~legaia-portrait-light");
                lightGo.hideFlags = HideFlags.HideAndDontSave;
                var l = lightGo.AddComponent<Light>();
                l.type = LightType.Directional;
                l.intensity = 1.15f;
                l.shadows = LightShadows.None;
                lightGo.transform.rotation =
                    Quaternion.LookRotation((-front + Vector3.down * 0.45f).normalized);

                rt = new RenderTexture(SIZE, SIZE, 24, RenderTextureFormat.ARGB32);
                rt.antiAliasing = 2;
                cam.targetTexture = rt;
                cam.Render();
                cam.targetTexture = null;

                RenderTexture.active = rt;
                var tex = new Texture2D(SIZE, SIZE, TextureFormat.RGBA32, false);
                tex.ReadPixels(new Rect(0f, 0f, SIZE, SIZE), 0, 0);
                tex.Apply();
                return tex;
            }
            finally
            {
                RenderTexture.active = prev;
                npc.position = keep;
                if (rt != null)
                {
                    rt.Release();
                    Object.DestroyImmediate(rt);
                }
                if (camGo != null)
                    Object.DestroyImmediate(camGo);
                if (lightGo != null)
                    Object.DestroyImmediate(lightGo);
            }
        }

        /// The direction the rig visibly faces, flattened - the locomotion
        /// controller's rest measurement, run here at build time (edit mode
        /// never evaluates the Animator, so the nodes still hold the glb
        /// rest pose the measurement is calibrated against).
        static Vector3 VisualFront(Transform npc)
        {
            Transform anchor = null, any = null;
            float bestUpright = -1f, bestAny = -1f;
            foreach (var mf in npc.GetComponentsInChildren<MeshFilter>())
            {
                Mesh mesh = mf.sharedMesh;
                if (mesh == null)
                    continue;
                Vector3 s = mesh.bounds.size;
                float score = s.x * s.y * s.z + mesh.vertexCount * 1e-6f;
                Transform t = mf.transform;
                Vector3 up = (t.TransformPoint(Vector3.up)
                    - t.TransformPoint(Vector3.zero)).normalized;
                if (score > bestAny)
                {
                    bestAny = score;
                    any = t;
                }
                if (up.y > 0.9f && score > bestUpright)
                {
                    bestUpright = score;
                    anchor = t;
                }
            }
            if (anchor == null)
                anchor = any;
            Vector3 f;
            if (anchor == null)
            {
                f = npc.forward;
            }
            else
            {
                // The face axis is +Z in the INSTANCE frame at rest; the
                // anchor node's own rest rotation is folded out, then the
                // whole matrix (mirrors included) is applied.
                Vector3 faceLocal = Quaternion.Inverse(anchor.rotation)
                    * (npc.rotation * Vector3.forward);
                f = anchor.TransformPoint(faceLocal) - anchor.TransformPoint(Vector3.zero);
            }
            f.y = 0f;
            return f.sqrMagnitude < 1e-8f ? Vector3.forward : f.normalized;
        }

        // --- the drawn fallback ---------------------------------------------------

        /// A flat head-and-shoulders silhouette, hue from the villager's own
        /// name - no graphics device involved, and two villagers never come
        /// out the same colour by accident.
        static Texture2D Silhouette(string token)
        {
            int h = 17;
            foreach (char c in token)
                h = h * 31 + c;
            if (h < 0)
                h = -h;
            Color body = Color.HSVToRGB((h % 360) / 360f, 0.42f,
                0.55f + (h / 360 % 30) * 0.008f);
            Color back = new Color(0.10f, 0.10f, 0.13f, 1f);

            var tex = new Texture2D(SIZE, SIZE, TextureFormat.RGBA32, false);
            var px = new Color[SIZE * SIZE];
            float headR = SIZE * 0.22f;
            Vector2 headC = new Vector2(SIZE * 0.5f, SIZE * 0.62f);
            for (int y = 0; y < SIZE; y++)
                for (int x = 0; x < SIZE; x++)
                {
                    float dx = x - headC.x, dy = y - headC.y;
                    bool head = dx * dx + dy * dy <= headR * headR;
                    // Shoulders: a wide ellipse rising from the bottom edge.
                    float sx = (x - SIZE * 0.5f) / (SIZE * 0.42f);
                    float sy = (y - SIZE * 0.12f) / (SIZE * 0.34f);
                    bool shoulder = y < SIZE * 0.42f && sx * sx + sy * sy <= 1f;
                    px[y * SIZE + x] = head || shoulder ? body : back;
                }
            tex.SetPixels(px);
            tex.Apply();
            return tex;
        }
    }
}
