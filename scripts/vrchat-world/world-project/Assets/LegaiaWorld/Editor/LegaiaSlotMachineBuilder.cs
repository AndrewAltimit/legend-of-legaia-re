// Editor menu `Legaia > Build Slot Machine...`: assembles the playable casino
// slot minigame onto a cabinet mesh, from the art `asset slot-art` exports
// (see docs/subsystems/minigame-slot-machine.md and the repo's
// scripts/vrchat-world/README.md).
//
//   asset slot-art extracted/PROT/0975_*.BIN extracted/PROT/1200_*.BIN \
//       --out "<unity project>/Assets/LegaiaImports/slot-art"
//
// What gets built, in the retail scene's own model units (the rig root is
// scaled so the 1280-unit glass width becomes `screenWidth` metres; local
// frame: x right, y up = -PSX y, z toward the player = -PSX z):
//
// - three 20-face reel cylinders (one quad per strip row, each face's
//   material = its display-strip value), spun by LegaiaSlotMachine. Retail
//   draws 8 faces at a 22.5 degree pitch and re-derives them per frame; a
//   rigid 20-face wheel at 18 degrees per row is the draft approximation -
//   same strip, same payline, slightly gentler curl;
// - the glass furniture from the disc's own tables (slot-machine.json):
//   5 payline lines, 5 lamps, 5 medallions, 3 reel-stop pedestals, the
//   marquee panel + mascots;
// - the dot-matrix marquee as message-bank quads (tally / pips / payout
//   caption / scrolled attract legend) - placement-level port of the retail
//   composer, not a per-dot grid;
// - a balance + status TextMesh HUD and the paytable board;
// - the three cabinet buttons wired as LegaiaSlotButton interacts (named
//   nodes on the cabinet mesh; fallback pads are built when missing).
//
// Everything the builder generates is from-scratch; the textures it consumes
// are the user's own disc-decoded exports (never committed).

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    public class LegaiaSlotMachineBuilder : EditorWindow
    {
        const string RIG_NAME = "LegaiaSlotGame";
        const string GEN_DIR = "Assets/LegaiaGenerated/slot-machine";
        // The glass spans x -640..+640 in model units (the payline table).
        const float GLASS_SPAN = 1280f;
        const int STRIP_LEN = 20;
        const float REEL_RADIUS = 540f; // between retail's 585 y / 512 z ellipse

        GameObject cabinet;
        string artDir = "Assets/LegaiaImports/slot-art";
        float screenWidth = 0.55f;
        Vector3 rigOffset = new Vector3(0f, 1.25f, 0.1f);
        float rigYaw;
        string buttonNames = "Circle.001;Circle.002;Circle.003";
        bool flipButtonOrder;
        bool buildPaytable = true;
        bool buildFallbackButtons = true;

        [MenuItem("Legaia/Build Slot Machine...")]
        static void Open()
        {
            GetWindow<LegaiaSlotMachineBuilder>("Legaia Slot Machine");
        }

        void OnGUI()
        {
            GUILayout.Label("Casino slot machine (minigame port)", EditorStyles.boldLabel);
            EditorGUILayout.HelpBox(
                "Feed: the `asset slot-art` export (PNGs + slot-machine.json). " +
                "The rig is built under the cabinet mesh; drag the rig root " +
                "afterwards to line the reels up with the screen cutout, then " +
                "copy its transform back into these fields so a rebuild lands " +
                "in the same place.", MessageType.Info);
            cabinet = (GameObject)EditorGUILayout.ObjectField(
                "Cabinet root", cabinet, typeof(GameObject), true);
            artDir = EditorGUILayout.TextField("Art folder", artDir);
            screenWidth = EditorGUILayout.FloatField(
                new GUIContent("Glass width (m)",
                    "Metres the 1280-unit payline span maps to."), screenWidth);
            rigOffset = EditorGUILayout.Vector3Field(
                new GUIContent("Rig local position",
                    "Local position of the game rig under the cabinet root."), rigOffset);
            rigYaw = EditorGUILayout.FloatField(
                new GUIContent("Rig yaw",
                    "Local Y rotation; the rig's +Z faces the player."), rigYaw);
            buttonNames = EditorGUILayout.TextField(
                new GUIContent("Button nodes",
                    "';'-separated node names on the cabinet mesh, wired left to right."),
                buttonNames);
            flipButtonOrder = EditorGUILayout.Toggle("Flip button order", flipButtonOrder);
            buildFallbackButtons = EditorGUILayout.Toggle(
                new GUIContent("Fallback buttons",
                    "Build simple pads when a named node is missing (stale mesh)."),
                buildFallbackButtons);
            buildPaytable = EditorGUILayout.Toggle("Paytable board", buildPaytable);
            GUILayout.Space(8);
            using (new EditorGUI.DisabledScope(!Directory.Exists(artDir)))
            {
                if (GUILayout.Button("Build slot machine", GUILayout.Height(28)))
                    Build();
            }
            if (!Directory.Exists(artDir))
                EditorGUILayout.HelpBox("Art folder not found: " + artDir, MessageType.Warning);
        }

        // ------------------------------------------------------------------

        void Build()
        {
            string manifestPath = artDir + "/slot-machine.json";
            if (!File.Exists(manifestPath))
            {
                Debug.LogError("[Legaia] no slot-machine.json in " + artDir +
                    " - run `asset slot-art` first.");
                return;
            }
            object m = MiniJson.Parse(File.ReadAllText(manifestPath));
            Directory.CreateDirectory(GEN_DIR);
            LegaiaWorldBuilder.EnsureUdonProgramAssets();

            // Replace an existing rig in place.
            Transform parent = cabinet != null ? cabinet.transform : null;
            var old = parent != null
                ? parent.Find(RIG_NAME)
                : (GameObject.Find(RIG_NAME) is GameObject g ? g.transform : null);
            if (old != null)
                Undo.DestroyObjectImmediate(old.gameObject);

            var rig = new GameObject(RIG_NAME);
            Undo.RegisterCreatedObjectUndo(rig, "Build Legaia slot machine");
            if (parent != null)
                rig.transform.SetParent(parent, false);
            rig.transform.localPosition = rigOffset;
            rig.transform.localRotation = Quaternion.Euler(0f, rigYaw, 0f);
            rig.transform.localScale = Vector3.one * (screenWidth / GLASS_SPAN);

            Mesh quad = EnsureQuadMesh();

            // --- materials over the exported art -------------------------
            var valueMats = new Material[20];
            for (int s = 0; s < 10; s++)
                valueMats[s] = CutoutMat("symbol_" + s, "symbols/symbol_" + s + ".png", false);
            for (int n = 1; n <= 10; n++)
                valueMats[9 + n] = CutoutMat("numeral_" + n, "symbols/numeral_" + n + ".png", false);
            var lampLit = CutoutMat("lamp_lit", "furniture/lamp_lit.png", false);
            var lampUnlit = CutoutMat("lamp_unlit", "furniture/lamp_unlit.png", false);
            var pedSpin = new Material[3];
            var pedStop = new Material[3];
            for (int r = 0; r < 3; r++)
            {
                pedSpin[r] = CutoutMat("pedestal_" + r + "_spin",
                    "furniture/pedestal_" + r + "_spin.png", false);
                pedStop[r] = CutoutMat("pedestal_" + r + "_stop",
                    "furniture/pedestal_" + r + "_stop.png", false);
            }
            var msgMats = new Material[21];
            for (int i = 0; i < 21; i++)
                msgMats[i] = CutoutMat("msg_" + i.ToString("00"),
                    "marquee/msg_" + i.ToString("00") + "_a.png", true);

            // --- the reels ------------------------------------------------
            var reelsRoot = Child(rig.transform, "reels", Vector3.zero);
            var reelPivots = new Transform[3];
            var reelFaces = new MeshRenderer[60];
            float chord = 2f * REEL_RADIUS * Mathf.Sin(Mathf.PI / STRIP_LEN) * 1.02f;
            float reelX0 = MiniJson.GetNum(m, "reel_x0", -512f);
            float reelXStep = MiniJson.GetNum(m, "reel_x_step", 384f);
            float reelW = MiniJson.GetNum(m, "reel_width", 256f);
            for (int r = 0; r < 3; r++)
            {
                var pivot = Child(reelsRoot, "reel_" + r,
                    new Vector3(reelX0 + r * reelXStep + reelW * 0.5f, 0f, 0f));
                reelPivots[r] = pivot;
                for (int row = 0; row < STRIP_LEN; row++)
                {
                    float theta = row * (360f / STRIP_LEN);
                    float rad = theta * Mathf.Deg2Rad;
                    var face = new GameObject("row_" + row);
                    face.transform.SetParent(pivot, false);
                    face.transform.localPosition = new Vector3(
                        0f, Mathf.Sin(rad) * REEL_RADIUS, Mathf.Cos(rad) * REEL_RADIUS);
                    face.transform.localRotation = Quaternion.Euler(-theta, 0f, 0f);
                    face.transform.localScale = new Vector3(reelW, chord, 1f);
                    face.AddComponent<MeshFilter>().sharedMesh = quad;
                    var mr = face.AddComponent<MeshRenderer>();
                    mr.sharedMaterial = valueMats[0];
                    mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
                    reelFaces[r * STRIP_LEN + row] = mr;
                }
            }
            // A black backdrop behind the wheels so the cabinet interior
            // never shows through, and a top/bottom shade in front of them -
            // retail's depth-cued gouraud fade, as a gradient overlay.
            MakeQuad(reelsRoot, "backdrop", quad, new Vector3(0f, 0f, -620f),
                new Vector2(1500f, 1400f), BlackMat());
            MakeQuad(reelsRoot, "shade", quad, new Vector3(0f, 0f, 660f),
                new Vector2(1500f, 3.4f * chord), ShadeMat());

            // --- the glass furniture -------------------------------------
            var glass = Child(rig.transform, "glass", Vector3.zero);
            var lampRenderers = new MeshRenderer[5];
            var lamps = MiniJson.AsList(MiniJson.Get(m, "lamps"));
            float lampW = 2f * MiniJson.GetNum(m, "lamp_half_w", 180f);
            float lampH = 2f * MiniJson.GetNum(m, "lamp_half_h", 160f);
            for (int i = 0; lamps != null && i < lamps.Count && i < 5; i++)
                lampRenderers[i] = MakeQuad(glass, "lamp_" + i, quad,
                    PsxPos(lamps[i], 2f), new Vector2(lampW, lampH), lampUnlit);
            var medallions = MiniJson.AsList(MiniJson.Get(m, "medallions"));
            float medW = 2f * MiniJson.GetNum(m, "medallion_half_w", 416f) * 0.5f;
            float medH = 2f * MiniJson.GetNum(m, "medallion_half_h", 208f) * 0.5f;
            for (int i = 0; medallions != null && i < medallions.Count && i < 5; i++)
                MakeQuad(glass, "medallion_" + i, quad,
                    PsxPos(MiniJson.Get(medallions[i], "pos"), 2f),
                    new Vector2(medW, medH),
                    CutoutMat("medallion_" + i, "furniture/medallion_" + i + ".png", false));
            var pedestalRenderers = new MeshRenderer[3];
            float pedX0 = MiniJson.GetNum(m, "pedestal_x0", -384f);
            float pedXStep = MiniJson.GetNum(m, "pedestal_x_step", 384f);
            float pedY = MiniJson.GetNum(m, "pedestal_y", 480f);
            for (int r = 0; r < 3; r++)
                pedestalRenderers[r] = MakeQuad(glass, "pedestal_" + r, quad,
                    new Vector3(pedX0 + r * pedXStep, -pedY, 801f),
                    new Vector2(340f, 200f), pedSpin[r]);
            var marqueeBbs = MiniJson.AsList(MiniJson.Get(m, "marquee"));
            for (int i = 0; marqueeBbs != null && i < marqueeBbs.Count && i < 3; i++)
            {
                var bb = marqueeBbs[i];
                MakeQuad(glass, "marquee_bb_" + i, quad,
                    PsxPos(MiniJson.Get(bb, "pos"), -1f),
                    new Vector2(2f * MiniJson.GetNum(bb, "half_w", 100f),
                                2f * MiniJson.GetNum(bb, "half_h", 100f)),
                    CutoutMat("marquee_" + i, "furniture/marquee_" + i + ".png", false));
            }

            // Paylines: the disc's five 3D segments, lit by the behaviour.
            var paylineLines = new LineRenderer[5];
            var paylines = MiniJson.AsList(MiniJson.Get(m, "paylines"));
            var lineMat = LineMat();
            float worldUnit = rig.transform.lossyScale.x;
            for (int i = 0; paylines != null && i < paylines.Count && i < 5; i++)
            {
                var go = new GameObject("payline_" + i);
                go.transform.SetParent(glass, false);
                var lr = go.AddComponent<LineRenderer>();
                lr.useWorldSpace = false;
                lr.positionCount = 2;
                lr.SetPosition(0, PsxPos(MiniJson.Get(paylines[i], "a"), 4f));
                lr.SetPosition(1, PsxPos(MiniJson.Get(paylines[i], "b"), 4f));
                lr.startWidth = 10f * worldUnit;
                lr.endWidth = 10f * worldUnit;
                lr.sharedMaterial = lineMat;
                lr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
                paylineLines[i] = lr;
            }

            // --- the dot-matrix marquee ----------------------------------
            // Anchor local frame: 1 unit = 1 dot column/row, origin at the
            // matrix top-left (what LegaiaSlotMachine's slot placement uses).
            float dotX0 = MiniJson.GetNum(m, "dot_x0", -429f);
            float dotY0 = MiniJson.GetNum(m, "dot_y0", -640f);
            float dotXStep = MiniJson.GetNum(m, "dot_x_step", 11f);
            float dotYStep = MiniJson.GetNum(m, "dot_y_step", 12f);
            var marquee = Child(rig.transform, "marquee", new Vector3(dotX0, -dotY0, 806f));
            marquee.localScale = new Vector3(dotXStep, dotYStep, 1f);
            var tallySlots = MarqueeSlots(marquee, quad, "tally", 3);
            var timesSlots = MarqueeSlots(marquee, quad, "times", 2);
            var pipSlots = MarqueeSlots(marquee, quad, "pip", 3);
            var payoutSlots = MarqueeSlots(marquee, quad, "payout", 4);
            var coinSlot = MarqueeSlots(marquee, quad, "coin", 1)[0];
            var legendQuad = MarqueeSlots(marquee, quad, "legend", 1)[0];
            float dotCols = MiniJson.GetNum(m, "dot_cols", 78f);
            float dotRows = MiniJson.GetNum(m, "dot_rows", 13f);
            legendQuad.transform.localPosition = new Vector3(dotCols * 0.5f, -dotRows * 0.5f, 0f);
            legendQuad.transform.localScale = new Vector3(dotCols, dotRows, 1f);
            legendQuad.sharedMaterial = msgMats[0];
            legendQuad.gameObject.SetActive(true);

            // --- HUD ------------------------------------------------------
            var hud = Child(rig.transform, "hud", Vector3.zero);
            if (buildPaytable)
                MakeQuad(hud, "paytable", quad, new Vector3(950f, 60f, 802f),
                    new Vector2(127f * 3.2f, 239f * 3.2f),
                    CutoutMat("paytable", "hud/paytable.png", false));
            var balanceText = MakeText(hud, "balance", new Vector3(0f, -760f, 810f), 60f);
            var statusText = MakeText(hud, "status", new Vector3(0f, -860f, 810f), 40f);

            // --- the machine behaviour -----------------------------------
            var machine = LegaiaWorldBuilder.TryAttachUdon(rig, "LegaiaSlotMachine");
            var W = new System.Action<string, object>(
                (field, value) => LegaiaWorldBuilder.SetUdonField(machine, field, value));
            W("reelPivots", reelPivots);
            W("reelFaces", reelFaces);
            W("valueMaterials", valueMats);
            W("lampRenderers", lampRenderers);
            W("lampLitMaterial", lampLit);
            W("lampUnlitMaterial", lampUnlit);
            W("paylineLines", paylineLines);
            W("pedestalRenderers", pedestalRenderers);
            W("pedestalSpinMaterials", pedSpin);
            W("pedestalStopMaterials", pedStop);
            W("tallySlots", tallySlots);
            W("timesSlots", timesSlots);
            W("pipSlots", pipSlots);
            W("payoutDigitSlots", payoutSlots);
            W("coinSlot", coinSlot);
            W("legendQuad", legendQuad);
            W("messageMaterials", msgMats);
            W("messageWidths", MessageWidths(m));
            W("balanceText", balanceText);
            W("statusText", statusText);
            W("payoutTable", IntArray(m, "payouts", 10));
            W("kickSymbol", (int)MiniJson.GetNum(m, "kick_symbol", 8f));
            W("kickRounds", (int)MiniJson.GetNum(m, "kick_rounds", 1f));
            W("punchSymbol", (int)MiniJson.GetNum(m, "punch_symbol", 9f));
            W("punchRounds", (int)MiniJson.GetNum(m, "punch_rounds", 3f));
            W("tallyNumberCols", IntArray(m, "tally_number_cols", 3));
            W("tallyTimesCols", IntArray(m, "tally_times_cols", 2));
            W("payoutDigitCols", IntArray(m, "payout_digit_cols", 4));
            W("payoutCoinsCol", (int)MiniJson.GetNum(m, "payout_coins_col", 52f));
            W("roundPipCols", IntArray(m, "round_pip_cols", 3));
            W("msgNumberBase", (int)MiniJson.GetNum(m, "msg_number_base", 6f));
            W("msgTimes", (int)MiniJson.GetNum(m, "msg_times", 17f));
            W("msgPipOn", (int)MiniJson.GetNum(m, "msg_pip_on", 18f));
            W("msgPipOff", (int)MiniJson.GetNum(m, "msg_pip_off", 19f));
            W("msgCoins", (int)MiniJson.GetNum(m, "msg_coins", 20f));
            W("dotCols", (int)dotCols);
            W("dotRows", (int)dotRows);
            LegaiaWorldBuilder.SyncUdonProxy(machine);

            WireButtons(rig, machine, worldUnit);

            Debug.Log("[Legaia] slot machine built under " +
                (parent != null ? parent.name : "scene root") +
                ". Drag '" + RIG_NAME + "' to line the reels up with the " +
                "screen cutout; test in ClientSim / Build & Test (Interact " +
                "the three buttons).");
        }

        // ------------------------------------------------------------------

        /// PSX table position -> rig local (y and z sign-flipped), with a
        /// small extra z lift to layer the glass quads without z-fighting.
        static Vector3 PsxPos(object pos, float zLift)
        {
            return new Vector3(
                MiniJson.GetNum(pos, "x", 0f),
                -MiniJson.GetNum(pos, "y", 0f),
                -MiniJson.GetNum(pos, "z", -800f) + zLift);
        }

        static Transform Child(Transform parent, string name, Vector3 localPos)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            return go.transform;
        }

        static MeshRenderer MakeQuad(Transform parent, string name, Mesh quad,
            Vector3 localPos, Vector2 size, Material mat)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            go.transform.localScale = new Vector3(size.x, size.y, 1f);
            go.AddComponent<MeshFilter>().sharedMesh = quad;
            var mr = go.AddComponent<MeshRenderer>();
            mr.sharedMaterial = mat;
            mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            return mr;
        }

        static MeshRenderer[] MarqueeSlots(Transform marquee, Mesh quad, string name, int count)
        {
            var slots = new MeshRenderer[count];
            for (int i = 0; i < count; i++)
            {
                var mr = MakeQuad(marquee, name + "_" + i, quad,
                    new Vector3(6.5f, -6.5f, 0f), new Vector2(13f, 13f), null);
                mr.gameObject.SetActive(false);
                slots[i] = mr;
            }
            return slots;
        }

        static TextMesh MakeText(Transform parent, string name, Vector3 localPos, float size)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            var tm = go.AddComponent<TextMesh>();
            tm.anchor = TextAnchor.MiddleCenter;
            tm.alignment = TextAlignment.Center;
            tm.fontSize = 64;
            tm.characterSize = size / 10f;
            tm.color = new Color(1f, 0.9f, 0.6f);
            tm.text = "";
            // A script-created TextMesh has no font and renders nothing
            // until one (and its material) is assigned.
            var font = Resources.GetBuiltinResource<Font>("LegacyRuntime.ttf");
            if (font == null)
                font = Resources.GetBuiltinResource<Font>("Arial.ttf");
            if (font != null)
            {
                tm.font = font;
                go.GetComponent<MeshRenderer>().sharedMaterial = font.material;
            }
            return tm;
        }

        float[] MessageWidths(object m)
        {
            var list = MiniJson.AsList(MiniJson.Get(m, "messages"));
            var widths = new float[21];
            for (int i = 0; i < widths.Length; i++)
                widths[i] = 13f;
            for (int i = 0; list != null && i < list.Count && i < widths.Length; i++)
                widths[i] = MiniJson.GetNum(list[i], "w", 13f);
            return widths;
        }

        static int[] IntArray(object m, string key, int count)
        {
            var list = MiniJson.AsList(MiniJson.Get(m, key));
            var arr = new int[count];
            for (int i = 0; list != null && i < list.Count && i < count; i++)
                arr[i] = (int)MiniJson.AsNum(list[i]);
            return arr;
        }

        // --- buttons -------------------------------------------------------

        void WireButtons(GameObject rig, Component machine, float worldUnit)
        {
            var names = buttonNames.Split(';');
            var found = new List<Transform>();
            Transform searchRoot = cabinet != null ? cabinet.transform : rig.transform;
            foreach (var raw in names)
            {
                string name = raw.Trim();
                if (name.Length == 0)
                    continue;
                var t = FindDeep(searchRoot, name);
                if (t != null)
                    found.Add(t);
                else
                    Debug.LogWarning("[Legaia] slot button node '" + name +
                        "' not found under " + searchRoot.name +
                        (buildFallbackButtons ? " - building a fallback pad." : "."));
            }
            // Left-to-right in the cabinet's own frame.
            found.Sort((a, b) =>
                searchRoot.InverseTransformPoint(a.position).x
                    .CompareTo(searchRoot.InverseTransformPoint(b.position).x));
            if (flipButtonOrder)
                found.Reverse();

            for (int i = 0; i < 3; i++)
            {
                GameObject go;
                if (i < found.Count)
                {
                    go = found[i].gameObject;
                    if (go.GetComponent<Collider>() == null)
                    {
                        var box = go.AddComponent<BoxCollider>();
                        var rend = go.GetComponentInChildren<Renderer>();
                        if (rend != null)
                        {
                            // Fit the collider to the rendered bounds, with a
                            // little extra depth so it's easy to point at.
                            var local = go.transform.InverseTransformPoint(rend.bounds.center);
                            box.center = local;
                            var ext = rend.bounds.size;
                            float inv = 1f / Mathf.Max(go.transform.lossyScale.x, 1e-5f);
                            box.size = new Vector3(ext.x, ext.y, ext.z * 2f + 0.02f) * inv;
                        }
                    }
                }
                else
                {
                    if (!buildFallbackButtons)
                        continue;
                    go = GameObject.CreatePrimitive(PrimitiveType.Cube);
                    go.name = "slot_button_" + i;
                    go.transform.SetParent(rig.transform, false);
                    go.transform.localPosition = new Vector3(-330f + i * 330f, -1000f, 900f);
                    go.transform.localScale = new Vector3(180f, 60f, 80f);
                }
                var btn = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaSlotButton");
                LegaiaWorldBuilder.SetUdonField(btn, "machine", machine);
                LegaiaWorldBuilder.SetUdonField(btn, "buttonIndex", i);
                LegaiaWorldBuilder.SyncUdonProxy(btn);
                SetInteractText(btn, i == 0 ? "Spin / Stop 1"
                    : i == 1 ? "Spin / Stop 2" : "Spin / Stop 3");
            }
        }

        /// Interaction prompt lives on the backing UdonBehaviour, not the
        /// U# proxy.
        static void SetInteractText(Component proxy, string text)
        {
            if (proxy == null)
                return;
            var util = LegaiaWorldBuilder.FindType("UdonSharpEditor.UdonSharpEditorUtility");
            var backing = util?.GetMethod("GetBackingUdonBehaviour")
                ?.Invoke(null, new object[] { proxy }) as Component;
            if (backing == null)
                return;
            var f = backing.GetType().GetField("interactText");
            if (f != null)
            {
                f.SetValue(backing, text);
                EditorUtility.SetDirty(backing);
            }
        }

        static Transform FindDeep(Transform root, string name)
        {
            if (root.name == name)
                return root;
            for (int i = 0; i < root.childCount; i++)
            {
                var hit = FindDeep(root.GetChild(i), name);
                if (hit != null)
                    return hit;
            }
            return null;
        }

        // --- generated assets ----------------------------------------------

        /// A unit quad in the XY plane whose front face looks along +Z (the
        /// rig's player side) - Unity's built-in Quad faces the other way.
        static Mesh EnsureQuadMesh()
        {
            string path = GEN_DIR + "/slot_quad.asset";
            var mesh = AssetDatabase.LoadAssetAtPath<Mesh>(path);
            if (mesh != null)
                return mesh;
            mesh = new Mesh { name = "slot_quad" };
            mesh.vertices = new[]
            {
                new Vector3(-0.5f, -0.5f, 0f), new Vector3(0.5f, -0.5f, 0f),
                new Vector3(0.5f, 0.5f, 0f), new Vector3(-0.5f, 0.5f, 0f),
            };
            mesh.uv = new[]
            {
                new Vector2(0f, 0f), new Vector2(1f, 0f),
                new Vector2(1f, 1f), new Vector2(0f, 1f),
            };
            mesh.triangles = new[] { 0, 3, 2, 0, 2, 1 };
            mesh.normals = new[]
                { Vector3.forward, Vector3.forward, Vector3.forward, Vector3.forward };
            mesh.RecalculateBounds();
            AssetDatabase.CreateAsset(mesh, path);
            return mesh;
        }

        Material CutoutMat(string name, string relTex, bool repeat)
        {
            string texPath = artDir + "/" + relTex;
            var tex = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            if (tex == null)
                Debug.LogWarning("[Legaia] slot art texture missing: " + texPath);
            else
                ConfigureTexture(texPath, repeat);
            string matPath = GEN_DIR + "/" + name + ".mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (mat == null)
            {
                mat = new Material(Shader.Find("Unlit/Transparent Cutout"));
                AssetDatabase.CreateAsset(mat, matPath);
            }
            mat.mainTexture = tex;
            if (mat.HasProperty("_Cutoff"))
                mat.SetFloat("_Cutoff", 0.5f);
            return mat;
        }

        /// PSX point sampling, no compression; the marquee messages wrap so
        /// the attract legend can scroll through its window.
        static void ConfigureTexture(string path, bool repeat)
        {
            var imp = AssetImporter.GetAtPath(path) as TextureImporter;
            if (imp == null)
                return;
            bool dirty = false;
            if (imp.filterMode != FilterMode.Point) { imp.filterMode = FilterMode.Point; dirty = true; }
            if (imp.textureCompression != TextureImporterCompression.Uncompressed)
            { imp.textureCompression = TextureImporterCompression.Uncompressed; dirty = true; }
            if (imp.mipmapEnabled) { imp.mipmapEnabled = false; dirty = true; }
            var wrap = repeat ? TextureWrapMode.Repeat : TextureWrapMode.Clamp;
            if (imp.wrapMode != wrap) { imp.wrapMode = wrap; dirty = true; }
            if (imp.npotScale != TextureImporterNPOTScale.None)
            { imp.npotScale = TextureImporterNPOTScale.None; dirty = true; }
            if (dirty)
                imp.SaveAndReimport();
        }

        static Material BlackMat()
        {
            string path = GEN_DIR + "/slot_black.mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (mat == null)
            {
                mat = new Material(Shader.Find("Unlit/Color"));
                AssetDatabase.CreateAsset(mat, path);
            }
            mat.color = Color.black;
            return mat;
        }

        static Material LineMat()
        {
            string path = GEN_DIR + "/slot_line.mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (mat == null)
            {
                mat = new Material(Shader.Find("Sprites/Default"));
                AssetDatabase.CreateAsset(mat, path);
            }
            return mat;
        }

        /// Vertical gradient: opaque black at the top and bottom edges,
        /// transparent through the middle band - the stand-in for retail's
        /// depth-cued reel shade (bright at the payline, black past ~48
        /// degrees either side).
        Material ShadeMat()
        {
            string texPath = GEN_DIR + "/slot_shade.png";
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                const int H = 128;
                var tex = new Texture2D(4, H, TextureFormat.RGBA32, false);
                for (int y = 0; y < H; y++)
                {
                    float t = Mathf.Abs(y / (H - 1f) - 0.5f) * 2f; // 0 centre, 1 edge
                    float a = Mathf.Clamp01((t - 0.35f) / 0.5f);
                    a = a * a;
                    for (int x = 0; x < 4; x++)
                        tex.SetPixel(x, y, new Color(0f, 0f, 0f, a));
                }
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
                var imp = AssetImporter.GetAtPath(texPath) as TextureImporter;
                if (imp != null)
                {
                    imp.wrapMode = TextureWrapMode.Clamp;
                    imp.SaveAndReimport();
                }
            }
            string matPath = GEN_DIR + "/slot_shade.mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (mat == null)
            {
                mat = new Material(Shader.Find("Unlit/Transparent"));
                AssetDatabase.CreateAsset(mat, matPath);
            }
            mat.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            return mat;
        }
    }
}
