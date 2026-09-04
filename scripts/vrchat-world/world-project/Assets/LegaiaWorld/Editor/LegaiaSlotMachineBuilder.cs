// Editor menu `Legaia > Build Slot Machine...`: assembles the playable casino
// slot minigame onto a cabinet mesh, from the art `asset slot-art` exports
// (see docs/subsystems/minigame-slot-machine.md and the repo's
// scripts/vrchat-world/README.md).
//
//   asset slot-art extracted/PROT/0975_*.BIN extracted/PROT/1200_*.BIN \
//       --out "<unity project>/Assets/LegaiaImports/slot-art"
//
// What gets built, in the retail scene's own model units (the studio node is
// scaled so the 1280-unit glass width becomes `screenWidth` metres; local
// frame: x right, y up = -PSX y, z toward the player = -PSX z):
//
// - three reel drums as retail draws them (FUN_801d0fa8): 8 face quads per
//   reel that LegaiaSlotMachine re-derives per frame at a 22.5-degree pitch
//   on the y=585 / z=512 ellipse. The faces use the LegaiaWorld/SlotReelFace
//   shader, which reproduces retail's depth-cue shade per pixel (bright at
//   the payline, black past ~48 degrees - the fade IS the top/bottom cap);
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
// Generated screen-feed assets (RenderTexture + screen material) are named
// per cabinet, so several machines in one scene don't fight over one feed.
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
        const int FACE_COUNT = 8;

        // Kit-level layout (not disc data): the paytable board and HUD text.
        static readonly Vector3 PAYTABLE_POS = new Vector3(950f, 60f, 802f);
        const float PAYTABLE_SCALE = 3.2f;
        static readonly Vector3 HUD_BALANCE_POS = new Vector3(0f, -760f, 810f);
        static readonly Vector3 HUD_STATUS_POS = new Vector3(0f, -860f, 810f);

        GameObject cabinet;
        string artDir = "Assets/LegaiaImports/slot-art";
        float screenWidth = 0.55f;
        Vector3 rigOffset = new Vector3(0f, 1.25f, 0.1f);
        float rigYaw;
        string buttonNames = "Circle.001;Circle.002;Circle.003";
        string screenNodeName = "screen";
        bool flipButtonOrder;
        bool buildPaytable = true;
        bool buildFallbackButtons = true;
        bool flatScreen = true;
        Vector2 viewCenter = new Vector2(80f, -150f);
        float viewHalfWidth = 1250f;

        // Studio-camera geometry (model units / derived from the unit scale).
        const float CAM_DIST = 3000f;
        const int RT_WIDTH = 640;
        const int RT_HEIGHT = 480;

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
                "afterwards to line the screen up with the cabinet, then " +
                "copy its transform back into these fields so a rebuild lands " +
                "in the same place.", MessageType.Info);
            cabinet = (GameObject)EditorGUILayout.ObjectField(
                "Cabinet root", cabinet, typeof(GameObject), true);
            artDir = EditorGUILayout.TextField("Art folder", artDir);
            flatScreen = EditorGUILayout.Toggle(
                new GUIContent("Flat screen",
                    "Film a hidden studio into a RenderTexture on a flat " +
                    "screen quad (recommended). Off = the 3D rig sits behind " +
                    "the cabinet's screen cutout instead."), flatScreen);
            screenWidth = EditorGUILayout.FloatField(
                new GUIContent("Screen width (m)",
                    "Metres across the visible screen (flat mode) or the " +
                    "1280-unit glass span (behind-glass mode)."), screenWidth);
            rigOffset = EditorGUILayout.Vector3Field(
                new GUIContent("Screen local position",
                    "Local position of the screen (or rig) under the cabinet root."),
                rigOffset);
            rigYaw = EditorGUILayout.FloatField(
                new GUIContent("Screen yaw",
                    "Local Y rotation; +Z faces the player."), rigYaw);
            if (flatScreen)
            {
                screenNodeName = EditorGUILayout.TextField(
                    new GUIContent("Screen node",
                        "Cabinet node whose flat face the screen quad snaps " +
                        "to. When found, the position/size fields above are " +
                        "derived from it (the face must look along the " +
                        "cabinet's +Z). Clear to place by hand."),
                    screenNodeName);
                viewCenter = EditorGUILayout.Vector2Field(
                    new GUIContent("View centre (model units)",
                        "What the studio camera looks at; 0,0 = the payline centre."),
                    viewCenter);
                viewHalfWidth = EditorGUILayout.FloatField(
                    new GUIContent("View half-width (model units)",
                        "Horizontal half-extent of the filmed frame (glass is 640)."),
                    viewHalfWidth);
            }
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

            // A cabinet node named after `screenNodeName` (e.g. the mesh's
            // own "screen" face) pins the build: position, yaw and width are
            // derived from its face instead of the hand-tuned fields. Runs
            // after the old rig is destroyed so it can't match the generated
            // quad of a previous build (also named "screen").
            TrySnapToScreenNode(parent);

            // Root: the visible anchor (screen quad, machine behaviour,
            // fallback buttons - all in metres). The machine itself is built
            // under a "studio" child in the retail scene's model units: at
            // the anchor in behind-glass mode, or parked 40 m below in flat
            // mode, where only the studio camera ever sees it.
            var rig = new GameObject(RIG_NAME);
            Undo.RegisterCreatedObjectUndo(rig, "Build Legaia slot machine");
            if (parent != null)
                rig.transform.SetParent(parent, false);
            rig.transform.localPosition = rigOffset;
            rig.transform.localRotation = Quaternion.Euler(0f, rigYaw, 0f);
            var studioGo = new GameObject("studio");
            studioGo.transform.SetParent(rig.transform, false);
            // The hidden-studio drop is 40 WORLD metres: the rig inherits the
            // cabinet's scale (a glb authored in centimetres often carries an
            // import scale), and a raw local drop would shrink with it and
            // leave the studio visibly inside the world.
            float drop = 40f / Mathf.Max(rig.transform.lossyScale.y, 1e-6f);
            studioGo.transform.localPosition =
                flatScreen ? new Vector3(0f, -drop, 0f) : Vector3.zero;
            studioGo.transform.localScale = Vector3.one * (screenWidth / GLASS_SPAN);
            Transform studio = studioGo.transform;
            float worldUnit = studio.lossyScale.x;

            Mesh quad = EnsureQuadMesh();

            // --- materials over the exported art -------------------------
            // Reel faces get the depth-cue shader; everything else is plain
            // unlit cutout.
            var faceShader = Shader.Find("LegaiaWorld/SlotReelFace");
            if (faceShader == null)
                Debug.LogWarning("[Legaia] LegaiaWorld/SlotReelFace shader not found " +
                    "(is Assets/LegaiaWorld/Shaders synced?) - reel faces fall " +
                    "back to unshaded cutout.");
            var valueMats = new Material[20];
            for (int s = 0; s < 10; s++)
                valueMats[s] = CutoutMat("symbol_" + s, "symbols/symbol_" + s + ".png",
                    false, faceShader);
            for (int n = 1; n <= 10; n++)
                valueMats[9 + n] = CutoutMat("numeral_" + n, "symbols/numeral_" + n + ".png",
                    false, faceShader);
            var msgMats = new Material[21];
            for (int i = 0; i < 21; i++)
                msgMats[i] = CutoutMat("msg_" + i.ToString("00"),
                    "marquee/msg_" + i.ToString("00") + "_a.png", true);

            var reelsRoot = Child(studio, "reels", Vector3.zero);
            var reelPivots = new Transform[3];
            var reelFaces = new MeshRenderer[3 * FACE_COUNT];
            float reelW = BuildReels(m, quad, reelsRoot, valueMats, reelPivots, reelFaces);
            BakeShadeVectors(studio, reelsRoot, valueMats);

            var lampRenderers = new MeshRenderer[5];
            var pedestalRenderers = new MeshRenderer[3];
            var paylineLines = new LineRenderer[5];
            Material lampLit, lampUnlit;
            Material[] pedSpin, pedStop;
            BuildGlass(m, quad, studio, worldUnit, lampRenderers, pedestalRenderers,
                paylineLines, out lampLit, out lampUnlit, out pedSpin, out pedStop);

            MeshRenderer[] tallySlots, timesSlots, pipSlots, payoutSlots;
            MeshRenderer coinSlot, legendQuad;
            float dotCols, dotRows;
            BuildMarquee(m, quad, studio, msgMats, out tallySlots, out timesSlots,
                out pipSlots, out payoutSlots, out coinSlot, out legendQuad,
                out dotCols, out dotRows);

            var hud = Child(studio, "hud", Vector3.zero);
            if (buildPaytable)
                MakeQuad(hud, "paytable", quad, PAYTABLE_POS,
                    new Vector2(127f * PAYTABLE_SCALE, 239f * PAYTABLE_SCALE),
                    CutoutMat("paytable", "hud/paytable.png", false));
            var balanceText = MakeText(hud, "balance", HUD_BALANCE_POS, 60f);
            var statusText = MakeText(hud, "status", HUD_STATUS_POS, 40f);

            Camera screenCamera;
            Transform screenPlane;
            BuildScreenFeed(rig, studio, quad, worldUnit,
                out screenCamera, out screenPlane);

            // --- the machine behaviour -----------------------------------
            var machine = LegaiaWorldBuilder.TryAttachUdon(rig, "LegaiaSlotMachine");
            var W = new System.Action<string, object>(
                (field, value) => LegaiaWorldBuilder.SetUdonField(machine, field, value));
            W("reelPivots", reelPivots);
            W("reelFaces", reelFaces);
            W("valueMaterials", valueMats);
            W("reelYRadius", MiniJson.GetNum(m, "reel_y_radius", 585f));
            W("reelZRadius", MiniJson.GetNum(m, "reel_z_radius", 512f));
            W("reelFaceWidth", reelW);
            W("reelShadeOrigin", reelsRoot);
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
            W("screenCamera", screenCamera);
            W("screenPlane", screenPlane);
            LegaiaWorldBuilder.SyncUdonProxy(machine);

            WireButtons(rig, machine, worldUnit);

            Debug.Log("[Legaia] slot machine built under " +
                (parent != null ? parent.name : "scene root") +
                ". Drag '" + RIG_NAME + "' to line the screen up with the " +
                "cabinet; test in ClientSim / Build & Test (Interact the " +
                "three buttons).");
        }

        // --- the reels ----------------------------------------------------

        /// Retail's drum: 8 face quads per reel that the machine re-derives
        /// per frame (angle base 0x380, 0x100 pitch, y/z ellipse). The
        /// builder places them at frac = 0 with placeholder symbols so the
        /// edit-mode preview shows the drum shape; returns the face width.
        float BuildReels(object m, Mesh quad, Transform reelsRoot,
            Material[] valueMats, Transform[] reelPivots, MeshRenderer[] reelFaces)
        {
            float reelX0 = MiniJson.GetNum(m, "reel_x0", -512f);
            float reelXStep = MiniJson.GetNum(m, "reel_x_step", 384f);
            float reelW = MiniJson.GetNum(m, "reel_width", 256f);
            float ry = MiniJson.GetNum(m, "reel_y_radius", 585f);
            float rz = MiniJson.GetNum(m, "reel_z_radius", 512f);
            for (int r = 0; r < 3; r++)
            {
                var pivot = Child(reelsRoot, "reel_" + r,
                    new Vector3(reelX0 + r * reelXStep + reelW * 0.5f, 0f, 0f));
                reelPivots[r] = pivot;
                for (int f = 0; f < FACE_COUNT; f++)
                {
                    var face = new GameObject("face_" + f);
                    face.transform.SetParent(pivot, false);
                    PlaceDrumFace(face.transform, f, ry, rz, reelW);
                    face.AddComponent<MeshFilter>().sharedMesh = quad;
                    var mr = face.AddComponent<MeshRenderer>();
                    mr.sharedMaterial = valueMats[f % 10];
                    mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
                    reelFaces[r * FACE_COUNT + f] = mr;
                }
            }
            // A black backdrop behind the drums so the cabinet interior never
            // shows through. (No shade overlay any more - the depth-cue fade
            // is the face shader's, as in retail.)
            MakeQuad(reelsRoot, "backdrop", quad, new Vector3(0f, 0f, -620f),
                new Vector2(1500f, 1400f), BlackMat());
            return reelW;
        }

        /// Face f's frac-0 pose: top edge at angle 0x380 + f*0x100 of a
        /// 0x1000 turn, on the y/z ellipse, in the rig frame (both PSX signs
        /// flipped). Mirror of LegaiaSlotMachine.ApplyReelVisuals.
        static void PlaceDrumFace(Transform face, int f, float ry, float rz, float w)
        {
            float step = Mathf.PI * 2f / 4096f;
            float aT = (0x380 + f * 0x100) * step;
            float aB = aT + 0x100 * step;
            float yT = Mathf.Sin(aT) * ry;
            float yB = Mathf.Sin(aB) * ry;
            float zT = -Mathf.Cos(aT) * rz;
            float zB = -Mathf.Cos(aB) * rz;
            float dy = yT - yB;
            float dz = zT - zB;
            face.localPosition = new Vector3(0f, (yT + yB) * 0.5f, (zT + zB) * 0.5f);
            face.localRotation = Quaternion.LookRotation(
                new Vector3(0f, -dz, dy), new Vector3(0f, dy, dz));
            face.localScale = new Vector3(w, Mathf.Sqrt(dy * dy + dz * dz), 1f);
        }

        /// Bake the face shader's world -> model-z reconstruction (the same
        /// pair LegaiaSlotMachine.BakeShadeVectors refreshes at Start).
        static void BakeShadeVectors(Transform studio, Transform reelsRoot, Material[] mats)
        {
            float unit = studio.lossyScale.z;
            if (unit < 1e-6f)
                return;
            Vector4 origin = reelsRoot.position;
            Vector4 axis = (Vector4)(studio.forward * (-1f / unit));
            foreach (var mat in mats)
            {
                if (mat == null || !mat.HasProperty("_ShadeOrigin"))
                    continue;
                mat.SetVector("_ShadeOrigin", origin);
                mat.SetVector("_ShadeAxis", axis);
                EditorUtility.SetDirty(mat);
            }
        }

        // --- the glass furniture ------------------------------------------

        void BuildGlass(object m, Mesh quad, Transform studio, float worldUnit,
            MeshRenderer[] lampRenderers, MeshRenderer[] pedestalRenderers,
            LineRenderer[] paylineLines, out Material lampLit, out Material lampUnlit,
            out Material[] pedSpin, out Material[] pedStop)
        {
            var glass = Child(studio, "glass", Vector3.zero);
            lampLit = CutoutMat("lamp_lit", "furniture/lamp_lit.png", false);
            lampUnlit = CutoutMat("lamp_unlit", "furniture/lamp_unlit.png", false);
            var lamps = MiniJson.AsList(MiniJson.Get(m, "lamps"));
            float lampW = 2f * MiniJson.GetNum(m, "lamp_half_w", 180f);
            float lampH = 2f * MiniJson.GetNum(m, "lamp_half_h", 160f);
            for (int i = 0; lamps != null && i < lamps.Count && i < 5; i++)
                lampRenderers[i] = MakeQuad(glass, "lamp_" + i, quad,
                    PsxPos(lamps[i], 2f), new Vector2(lampW, lampH), lampUnlit);
            var medallions = MiniJson.AsList(MiniJson.Get(m, "medallions"));
            float medW = MiniJson.GetNum(m, "medallion_half_w", 416f);
            float medH = MiniJson.GetNum(m, "medallion_half_h", 208f);
            for (int i = 0; medallions != null && i < medallions.Count && i < 5; i++)
                MakeQuad(glass, "medallion_" + i, quad,
                    PsxPos(MiniJson.Get(medallions[i], "pos"), 2f),
                    new Vector2(medW, medH),
                    CutoutMat("medallion_" + i, "furniture/medallion_" + i + ".png", false));
            pedSpin = new Material[3];
            pedStop = new Material[3];
            float pedX0 = MiniJson.GetNum(m, "pedestal_x0", -384f);
            float pedXStep = MiniJson.GetNum(m, "pedestal_x_step", 384f);
            float pedY = MiniJson.GetNum(m, "pedestal_y", 480f);
            float pedW = 2f * MiniJson.GetNum(m, "pedestal_half_w", 170f);
            float pedH = 2f * MiniJson.GetNum(m, "pedestal_half_h", 100f);
            for (int r = 0; r < 3; r++)
            {
                pedSpin[r] = CutoutMat("pedestal_" + r + "_spin",
                    "furniture/pedestal_" + r + "_spin.png", false);
                pedStop[r] = CutoutMat("pedestal_" + r + "_stop",
                    "furniture/pedestal_" + r + "_stop.png", false);
                pedestalRenderers[r] = MakeQuad(glass, "pedestal_" + r, quad,
                    new Vector3(pedX0 + r * pedXStep, -pedY, 801f),
                    new Vector2(pedW, pedH), pedSpin[r]);
            }
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
            var paylines = MiniJson.AsList(MiniJson.Get(m, "paylines"));
            var lineMat = LineMat();
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
        }

        // --- the dot-matrix marquee ---------------------------------------

        /// Anchor local frame: 1 unit = 1 dot column/row, origin at the
        /// matrix top-left (what LegaiaSlotMachine's slot placement uses).
        void BuildMarquee(object m, Mesh quad, Transform studio, Material[] msgMats,
            out MeshRenderer[] tallySlots, out MeshRenderer[] timesSlots,
            out MeshRenderer[] pipSlots, out MeshRenderer[] payoutSlots,
            out MeshRenderer coinSlot, out MeshRenderer legendQuad,
            out float dotCols, out float dotRows)
        {
            float dotX0 = MiniJson.GetNum(m, "dot_x0", -429f);
            float dotY0 = MiniJson.GetNum(m, "dot_y0", -640f);
            float dotXStep = MiniJson.GetNum(m, "dot_x_step", 11f);
            float dotYStep = MiniJson.GetNum(m, "dot_y_step", 12f);
            var marquee = Child(studio, "marquee", new Vector3(dotX0, -dotY0, 806f));
            marquee.localScale = new Vector3(dotXStep, dotYStep, 1f);
            tallySlots = MarqueeSlots(marquee, quad, "tally", 3);
            timesSlots = MarqueeSlots(marquee, quad, "times", 2);
            pipSlots = MarqueeSlots(marquee, quad, "pip", 3);
            payoutSlots = MarqueeSlots(marquee, quad, "payout", 4);
            coinSlot = MarqueeSlots(marquee, quad, "coin", 1)[0];
            legendQuad = MarqueeSlots(marquee, quad, "legend", 1)[0];
            dotCols = MiniJson.GetNum(m, "dot_cols", 78f);
            dotRows = MiniJson.GetNum(m, "dot_rows", 13f);
            legendQuad.transform.localPosition = new Vector3(dotCols * 0.5f, -dotRows * 0.5f, 0f);
            legendQuad.transform.localScale = new Vector3(dotCols, dotRows, 1f);
            legendQuad.sharedMaterial = msgMats[0];
            legendQuad.gameObject.SetActive(true);
        }

        // --- the screen feed (flat mode) ----------------------------------

        /// A camera films the hidden studio into a RenderTexture shown on a
        /// flat quad at the anchor - the machine is filmed, not modelled on
        /// the glass. The quad's horizontal UV is flipped: the camera faces
        /// the machine, so its image mirrors model +x, while retail projects
        /// +x to screen-right (lamps on the right).
        void BuildScreenFeed(GameObject rig, Transform studio, Mesh quad,
            float worldUnit, out Camera screenCamera, out Transform screenPlane)
        {
            screenCamera = null;
            screenPlane = null;
            if (!flatScreen)
                return;
            var rt = EnsureScreenRT();
            var camGo = new GameObject("screen_camera");
            camGo.transform.SetParent(studio, false);
            camGo.transform.localPosition =
                new Vector3(viewCenter.x, viewCenter.y, CAM_DIST);
            camGo.transform.localRotation = Quaternion.Euler(0f, 180f, 0f);
            screenCamera = camGo.AddComponent<Camera>();
            screenCamera.clearFlags = CameraClearFlags.SolidColor;
            screenCamera.backgroundColor = Color.black;
            float halfH = viewHalfWidth * ((float)RT_HEIGHT / RT_WIDTH);
            screenCamera.fieldOfView =
                2f * Mathf.Atan(halfH / CAM_DIST) * Mathf.Rad2Deg;
            // Clip tightly around the studio so nothing else underground
            // can wander into frame; distances scale with the unit size.
            screenCamera.nearClipPlane = Mathf.Max(0.01f, 1000f * worldUnit);
            screenCamera.farClipPlane = 4500f * worldUnit;
            screenCamera.allowHDR = false;
            screenCamera.allowMSAA = false;
            screenCamera.useOcclusionCulling = false;
            screenCamera.targetTexture = rt;

            var screenMr = MakeQuad(rig.transform, "screen", quad, Vector3.zero,
                new Vector2(screenWidth, screenWidth * ((float)RT_HEIGHT / RT_WIDTH)),
                ScreenMat(rt));
            screenPlane = screenMr.transform;
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

        // --- screen-node snap ----------------------------------------------

        /// Fit the build to a cabinet node that IS the screen (a flat face
        /// looking along the cabinet's +Z): rig offset, yaw and screen width
        /// are derived from its mesh corners in cabinet space, so a mesh
        /// swap is "select cabinet, Build" with no hand alignment. Writes
        /// the derived values back into the window fields (visible after
        /// the build, and used verbatim by a later manual rebuild).
        void TrySnapToScreenNode(Transform parent)
        {
            if (!flatScreen || parent == null)
                return;
            string name = screenNodeName != null ? screenNodeName.Trim() : "";
            if (name.Length == 0)
                return;
            var node = FindDeep(parent, name);
            var mf = node != null ? node.GetComponent<MeshFilter>() : null;
            if (mf == null || mf.sharedMesh == null)
            {
                if (node != null)
                    Debug.LogWarning("[Legaia] screen node '" + name +
                        "' has no mesh - placing the screen by hand instead.");
                return;
            }

            // The face's 8 mesh-bounds corners, in cabinet-local space (the
            // frame the rig's own offset lives in - so whatever import scale
            // or scene placement the cabinet carries is respected).
            Bounds b = mf.sharedMesh.bounds;
            var min = new Vector3(float.MaxValue, float.MaxValue, float.MaxValue);
            var max = new Vector3(float.MinValue, float.MinValue, float.MinValue);
            for (int i = 0; i < 8; i++)
            {
                var corner = new Vector3(
                    (i & 1) == 0 ? b.min.x : b.max.x,
                    (i & 2) == 0 ? b.min.y : b.max.y,
                    (i & 4) == 0 ? b.min.z : b.max.z);
                var local = parent.InverseTransformPoint(node.TransformPoint(corner));
                min = Vector3.Min(min, local);
                max = Vector3.Max(max, local);
            }
            float w = max.x - min.x;
            float h = max.y - min.y;
            if (w <= 0f || h <= 0f)
            {
                Debug.LogWarning("[Legaia] screen node '" + name +
                    "' has a degenerate face - placing the screen by hand instead.");
                return;
            }
            // Centre of the face, nudged just in front of it so the feed
            // quad covers the cabinet's own baked screen material.
            screenWidth = w;
            rigOffset = new Vector3(
                (min.x + max.x) * 0.5f,
                (min.y + max.y) * 0.5f,
                max.z + w * 0.01f);
            rigYaw = 0f;
            Debug.Log("[Legaia] screen snapped to node '" + name + "': " +
                w.ToString("0.###") + " x " + h.ToString("0.###") +
                " (cabinet units; feed quad is 4:3 " + w.ToString("0.###") + " x " +
                (w * 0.75f).ToString("0.###") + ") at " + rigOffset + ".");
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
                    // Metres, under the (unscaled) anchor root: a row of pads
                    // just below the screen.
                    go = GameObject.CreatePrimitive(PrimitiveType.Cube);
                    go.name = "slot_button_" + i;
                    go.transform.SetParent(rig.transform, false);
                    go.transform.localPosition = new Vector3(
                        -0.17f + i * 0.17f,
                        -(screenWidth * 0.375f + 0.07f), 0.03f);
                    go.transform.localScale = new Vector3(0.09f, 0.04f, 0.05f);
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

        /// Per-cabinet suffix for the screen-feed assets, so two machines in
        /// one scene never share a RenderTexture (two cameras writing one
        /// texture fight - each cabinet gets its own feed).
        string RigAssetSuffix()
        {
            string raw = cabinet != null ? cabinet.name : "scene";
            var sb = new System.Text.StringBuilder(raw.Length);
            foreach (char c in raw)
                sb.Append(char.IsLetterOrDigit(c) ? c : '_');
            return sb.ToString();
        }

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

        Material CutoutMat(string name, string relTex, bool repeat, Shader shader = null)
        {
            string texPath = artDir + "/" + relTex;
            var tex = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            if (tex == null)
                Debug.LogWarning("[Legaia] slot art texture missing: " + texPath);
            else
                ConfigureTexture(texPath, repeat);
            if (shader == null)
                shader = Shader.Find("Unlit/Transparent Cutout");
            string matPath = GEN_DIR + "/" + name + ".mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (mat == null)
            {
                mat = new Material(shader);
                AssetDatabase.CreateAsset(mat, matPath);
            }
            else if (mat.shader != shader)
            {
                mat.shader = shader; // upgrade a material built before the shader change
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

        /// The screen feed target: a PSX-ish low-res RenderTexture, point
        /// sampled so the pixels stay crisp on the cabinet screen. One per
        /// cabinet (see RigAssetSuffix).
        RenderTexture EnsureScreenRT()
        {
            string path = GEN_DIR + "/slot_screen_" + RigAssetSuffix() + ".renderTexture";
            var rt = AssetDatabase.LoadAssetAtPath<RenderTexture>(path);
            if (rt == null)
            {
                rt = new RenderTexture(RT_WIDTH, RT_HEIGHT, 16)
                {
                    name = "slot_screen",
                    antiAliasing = 1,
                };
                AssetDatabase.CreateAsset(rt, path);
            }
            rt.filterMode = FilterMode.Point;
            return rt;
        }

        /// Unlit screen material over the feed, horizontally flipped (see the
        /// screen-feed comment in BuildScreenFeed).
        Material ScreenMat(RenderTexture rt)
        {
            string path = GEN_DIR + "/slot_screen_" + RigAssetSuffix() + ".mat";
            var mat = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (mat == null)
            {
                mat = new Material(Shader.Find("Unlit/Texture"));
                AssetDatabase.CreateAsset(mat, path);
            }
            mat.mainTexture = rt;
            mat.mainTextureScale = new Vector2(-1f, 1f);
            mat.mainTextureOffset = new Vector2(1f, 0f);
            return mat;
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
    }
}
