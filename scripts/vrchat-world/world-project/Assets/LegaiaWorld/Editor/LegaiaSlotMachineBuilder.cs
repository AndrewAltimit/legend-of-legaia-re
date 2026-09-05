// Editor menu `Legaia > Build Slot Machine...`: assembles the playable casino
// slot minigame onto a cabinet mesh, from the art `asset slot-art` exports
// (see docs/subsystems/minigame-slot-machine.md and the repo's
// scripts/vrchat-world/README.md).
//
//   asset slot-art extracted/PROT/0975_*.BIN extracted/PROT/1200_*.BIN \
//       --out "<unity project>/Assets/LegaiaImports/slot-art"
//
// What gets built, in the retail scene's own model units (local frame:
// x right, y up = -PSX y, z toward the player = -PSX z). The whole
// composition is parented DIRECTLY onto the cabinet's screen face - no
// hidden studio, no camera, no RenderTexture. The composition root:
//
// - scales so the old filmed frame (view centre +- view half-width) spans
//   the screen node;
// - mirrors x once (negative x scale): a +z-facing quad reads mirrored to
//   the player looking at it, so exactly one flip is correct - which is
//   why every shader here is Cull Off (the mirror reverses winding);
// - flattens z by Z_FLATTEN, so the drum and overlay layers are millimetres
//   of relief on the cabinet face; overlay ordering is decided by material
//   renderQueue, not by the micrometre z gaps the flatten leaves;
// - bakes the retail camera's perspective in software: every static quad is
//   placed at k = D/(D - z) about the view centre, and LegaiaSlotMachine
//   applies the same k to the 8 drum faces per frame. Without it the lamps
//   and paylines (z ~800, k ~1.36) would misalign with the payline symbols
//   (z ~500, k ~1.2).
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
// - a balance + status TextMeshPro HUD and the paytable board;
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
        const int FACE_COUNT = 8;

        // The z flatten that turns the composition into screen-face relief.
        const float Z_FLATTEN = 0.02f;

        // renderQueue ladder: the flattened overlay layers draw in this
        // order (later wins at equal depth), replacing z separation.
        const int Q_BACKDROP = 2445;
        const int Q_REEL_FACE = 2450;
        const int Q_MARQUEE_BB = 2452;
        const int Q_PEDESTAL = 2453;
        const int Q_GLASS = 2454;   // lamps, medallions
        const int Q_MESSAGE = 2456;
        const int Q_SCREEN = 2458;  // retail's raw screen-space draws (paytable, HUD)

        // Kit-level layout (not disc data): where the balance / status text
        // sits, in retail screen-space PIXELS (the coin readout is a
        // screen-space HUD draw in retail too - FUN_801cfff0).
        static readonly Vector2 HUD_BALANCE_PX = new Vector2(280f, 208f);
        static readonly Vector2 HUD_STATUS_PX = new Vector2(280f, 226f);
        const float HUD_Z = 820f;

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
        bool matchWorldShading = true;

        // The retail projection, read from the manifest at build time
        // (minigame_slot_scene constants; defaults match the USA disc).
        // The composition frame is isotropic: the aspect-2 y term exactly
        // cancels the 640x240 framebuffer's 1:2 pixels, so only x needs a
        // scale and y follows.
        float projZ0 = 9324f;
        float projSx0 = 0.2547f;
        float projOfx = 253f;
        float projOfy = 118.5f;
        float projAspect = 2f;
        float projXScale = 6f;
        float screenPxW = 640f;
        float screenPxH = 240f;
        // Derived per build: the screen window in projected model units.
        Vector2 viewCenter = new Vector2(263f, -11.8f);
        float viewHalfWidth = 1256.4f;

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
            screenWidth = EditorGUILayout.FloatField(
                new GUIContent("Screen width",
                    "Width of the visible screen in cabinet-local units " +
                    "(derived from the screen node when one is found)."),
                screenWidth);
            rigOffset = EditorGUILayout.Vector3Field(
                new GUIContent("Screen local position",
                    "Local position of the screen centre under the cabinet root."),
                rigOffset);
            rigYaw = EditorGUILayout.FloatField(
                new GUIContent("Screen yaw",
                    "Local Y rotation; +Z faces the player."), rigYaw);
            screenNodeName = EditorGUILayout.TextField(
                new GUIContent("Screen node",
                    "Cabinet node whose flat face the composition snaps " +
                    "to. When found, the position/size fields above are " +
                    "derived from it (the face must look along the " +
                    "cabinet's +Z). Clear to place by hand."),
                screenNodeName);
            buttonNames = EditorGUILayout.TextField(
                new GUIContent("Button nodes",
                    "';'-separated node names on the cabinet mesh, wired " +
                    "left-to-right as the player facing the cabinet sees them."),
                buttonNames);
            flipButtonOrder = EditorGUILayout.Toggle("Flip button order", flipButtonOrder);
            buildFallbackButtons = EditorGUILayout.Toggle(
                new GUIContent("Fallback buttons",
                    "Build simple pads when a named node is missing (stale mesh)."),
                buildFallbackButtons);
            buildPaytable = EditorGUILayout.Toggle("Paytable board", buildPaytable);
            matchWorldShading = EditorGUILayout.Toggle(
                new GUIContent("Match world shading",
                    "Convert the cabinet's imported (glTFast PBR) materials " +
                    "to the kit's lit vertex-color shaders so it sits under " +
                    "the same sun and ambient as the rest of the scene. The " +
                    "screen composition stays unlit - it is a display."),
                matchWorldShading);
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

            // The retail projection (see minigame_slot_scene): scale
            // z0/(z0 - z_comp) about the model ORIGIN. The visible screen
            // window (what the 640x240 framebuffer showed) is derived from
            // the same constants - it is not centred on the origin.
            projZ0 = MiniJson.GetNum(m, "proj_z0", 9324f);
            projSx0 = MiniJson.GetNum(m, "proj_sx0", 0.2547f);
            projOfx = MiniJson.GetNum(m, "proj_ofx", 253f);
            projOfy = MiniJson.GetNum(m, "proj_ofy", 118.5f);
            projAspect = MiniJson.GetNum(m, "proj_aspect", 2f);
            projXScale = MiniJson.GetNum(m, "proj_xscale", 6f);
            screenPxW = MiniJson.GetNum(m, "screen_w", 640f);
            screenPxH = MiniJson.GetNum(m, "screen_h", 240f);
            viewHalfWidth = screenPxW * 0.5f / projSx0;
            viewCenter = new Vector2(
                (screenPxW * 0.5f - projOfx) / projSx0,
                -((screenPxH * 0.5f - projOfy) / (projSx0 / projAspect)));

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
            // after the old rig is destroyed so it can't match anything a
            // previous build generated.
            TrySnapToScreenNode(parent);

            // Shade the cabinet like the scene around it: its glTFast PBR
            // materials answer the sun and ambient probe on their own terms
            // and read overbright next to the kit-shaded world. Runs before
            // the rig exists, and skips every Legaia* material (the blacked
            // screen face, and any already-converted rebuild).
            if (matchWorldShading && cabinet != null)
                LegaiaRealism.ConvertPropToLit(cabinet, GEN_DIR);

            // Root: the anchor at the cabinet's screen face (behaviour +
            // fallback buttons, cabinet units). The composition is built
            // under a "screen_root" child, in the retail scene's model
            // units, directly on the face:
            //
            // - scale x is NEGATIVE - the single mirror that makes a
            //   +z-facing quad read correctly to a player looking at it
            //   (every shader in the build is Cull Off for this);
            // - scale z carries the flatten - the drum and overlays become
            //   millimetres of relief instead of a hologram;
            // - the position puts the view centre at the screen centre.
            var rig = new GameObject(RIG_NAME);
            Undo.RegisterCreatedObjectUndo(rig, "Build Legaia slot machine");
            if (parent != null)
                rig.transform.SetParent(parent, false);
            rig.transform.localPosition = rigOffset;
            rig.transform.localRotation = Quaternion.Euler(0f, rigYaw, 0f);
            var studioGo = new GameObject("screen_root");
            studioGo.transform.SetParent(rig.transform, false);
            float compScale = screenWidth / (2f * viewHalfWidth);
            studioGo.transform.localScale =
                new Vector3(-compScale, compScale, compScale * Z_FLATTEN);
            studioGo.transform.localPosition =
                new Vector3(compScale * viewCenter.x, -compScale * viewCenter.y, 0f);
            Transform studio = studioGo.transform;
            float worldUnit = Mathf.Abs(studio.lossyScale.x);

            Mesh quad = EnsureQuadMesh();

            // --- materials over the exported art -------------------------
            // Reel faces get the depth-cue shader; everything else is the
            // kit's Cull Off cutout (the mirror reverses winding, so the
            // legacy back-culled cutout would render nothing).
            var faceShader = Shader.Find("LegaiaWorld/SlotReelFace");
            if (faceShader == null)
                Debug.LogWarning("[Legaia] LegaiaWorld/SlotReelFace shader not found " +
                    "(is Assets/LegaiaWorld/Shaders synced?) - reel faces fall " +
                    "back to unshaded cutout AND will be invisible (backface" +
                    " culled under the mirrored composition).");
            var valueMats = new Material[20];
            for (int i = 0; i < 10; i++)
                valueMats[i] = CutoutMat("symbol_" + i, "symbols/symbol_" + i + ".png",
                    false, faceShader, Q_REEL_FACE);
            for (int n = 1; n <= 10; n++)
                valueMats[9 + n] = CutoutMat("numeral_" + n, "symbols/numeral_" + n + ".png",
                    false, faceShader, Q_REEL_FACE);
            var msgMats = new Material[21];
            for (int i = 0; i < 21; i++)
                msgMats[i] = CutoutMat("msg_" + i.ToString("00"),
                    "marquee/msg_" + i.ToString("00") + "_a.png", true, null, Q_MESSAGE);

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

            // The paytable is retail's raw screen-space draw: the 127x239
            // panel centred at framebuffer (560, 128), no projection.
            var hud = Child(studio, "hud", Vector3.zero);
            if (buildPaytable)
                MakeQuad(hud, "paytable", quad,
                    ScreenPx(MiniJson.GetNum(m, "paytable_px_x", 560f),
                             MiniJson.GetNum(m, "paytable_px_y", 128f), HUD_Z),
                    new Vector2(PxToModelW(127f), PxToModelH(239f)),
                    CutoutMat("paytable", "hud/paytable.png", false, null, Q_SCREEN));
            var balanceText = MakeText(hud, "balance",
                ScreenPx(HUD_BALANCE_PX.x, HUD_BALANCE_PX.y, HUD_Z), 90f);
            var statusText = MakeText(hud, "status",
                ScreenPx(HUD_STATUS_PX.x, HUD_STATUS_PX.y, HUD_Z), 65f);

            // Stale feed assets from a pre-direct-screen build (the studio
            // camera's RenderTexture + flipped screen material).
            AssetDatabase.DeleteAsset(GEN_DIR + "/slot_screen_" + RigAssetSuffix() + ".renderTexture");
            AssetDatabase.DeleteAsset(GEN_DIR + "/slot_screen_" + RigAssetSuffix() + ".mat");

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
            W("projectionDistance", projZ0);
            LegaiaWorldBuilder.SyncUdonProxy(machine);

            WireButtons(rig, machine, worldUnit);

            Debug.Log("[Legaia] slot machine built under " +
                (parent != null ? parent.name : "scene root") +
                ". Drag '" + RIG_NAME + "' to line the screen up with the " +
                "cabinet; test in ClientSim / Build & Test (Interact the " +
                "three buttons).");
        }

        // --- software projection ------------------------------------------

        /// The retail perspective at composition z (comp z = -PSX z, toward
        /// the player): `k = z0 / (z0 + z_psx)`. Baked into placements here,
        /// and applied per frame to the drum faces by LegaiaSlotMachine.
        float ProjK(float z)
        {
            return projZ0 / (projZ0 - z);
        }

        /// Project a composition-frame position: x/y scaled by k(z) about
        /// the model ORIGIN (the retail vanishing point - NOT the screen
        /// centre); z kept (the composition root's scale flattens it).
        Vector3 Proj(Vector3 p)
        {
            float k = ProjK(p.z);
            return new Vector3(p.x * k, p.y * k, p.z);
        }

        /// A retail screen-space pixel (the paytable / HUD draws) into the
        /// projected composition frame - these draws bypass the projection.
        Vector3 ScreenPx(float px, float py, float z)
        {
            return new Vector3(
                (px - projOfx) / projSx0,
                -((py - projOfy) / (projSx0 / projAspect)),
                z);
        }

        /// Framebuffer pixel spans into projected model units. y pixels are
        /// twice the model units of x pixels (the 640x240 grid's 1:2 shape).
        float PxToModelW(float px) { return px / projSx0; }
        float PxToModelH(float px) { return px / (projSx0 / projAspect); }

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
                    PlaceDrumFace(face.transform, f, ry, rz, reelW,
                        pivot.localPosition.x);
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
            var back = new Vector3(0f, 0f, -620f);
            MakeQuad(reelsRoot, "backdrop", quad, Proj(back),
                ProjK(back.z) * new Vector2(1500f, 1400f), BlackMat());
            return reelW;
        }

        /// Face f's frac-0 pose: top edge at angle 0x380 + f*0x100 of a
        /// 0x1000 turn, on the y/z ellipse, in the rig frame (both PSX signs
        /// flipped), through the software projection. Mirror of
        /// LegaiaSlotMachine.ApplyReelVisuals - runtime overwrites this on
        /// the first visual pass; this is the edit-mode preview.
        void PlaceDrumFace(Transform face, int f, float ry, float rz, float w,
            float pivotX)
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
            float yC = (yT + yB) * 0.5f;
            float zC = (zT + zB) * 0.5f;
            float k = ProjK(zC);
            // Projection about the model origin; the pivot itself sits
            // unprojected on the z=0 plane.
            face.localPosition = new Vector3(pivotX * (k - 1f), yC * k, zC);
            face.localRotation = Quaternion.LookRotation(
                new Vector3(0f, -dz, dy), new Vector3(0f, dy, dz));
            face.localScale =
                new Vector3(w * k, Mathf.Sqrt(dy * dy + dz * dz) * k, 1f);
        }

        /// Bake the face shader's world -> model-z reconstruction (the same
        /// pair LegaiaSlotMachine.BakeShadeVectors refreshes at Start).
        static void BakeShadeVectors(Transform studio, Transform reelsRoot, Material[] mats)
        {
            // lossyScale.z carries the composition's z flatten, so the
            // shader's reconstructed model z stays retail-exact (the
            // flatten cancels); Abs guards against the mirrored x leaking
            // a sign through a rotated setup.
            float unit = Mathf.Abs(studio.lossyScale.z);
            if (unit < 1e-9f) // the flatten makes this legitimately tiny
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
            // Billboard half-extents are VIEW space in the disc tables: the
            // retail projector builds the corners after the camera-matrix
            // multiply, so they divide by its x scale (6) - and y additionally
            // gains the aspect (2), because billboards are NOT aspect-
            // corrected while positions are. Model units = w/6, h/3.
            float vmx = 1f / projXScale;
            float vmy = projAspect / projXScale;

            var glass = Child(studio, "glass", Vector3.zero);
            lampLit = CutoutMat("lamp_lit", "furniture/lamp_lit.png", false, null, Q_GLASS);
            lampUnlit = CutoutMat("lamp_unlit", "furniture/lamp_unlit.png", false, null, Q_GLASS);
            var lamps = MiniJson.AsList(MiniJson.Get(m, "lamps"));
            float lampW = 2f * MiniJson.GetNum(m, "lamp_half_w", 180f) * vmx;
            float lampH = 2f * MiniJson.GetNum(m, "lamp_half_h", 160f) * vmy;
            for (int i = 0; lamps != null && i < lamps.Count && i < 5; i++)
            {
                var p = PsxPos(lamps[i], 2f);
                lampRenderers[i] = MakeQuad(glass, "lamp_" + i, quad,
                    Proj(p), ProjK(p.z) * new Vector2(lampW, lampH), lampUnlit);
            }
            var medallions = MiniJson.AsList(MiniJson.Get(m, "medallions"));
            float medW = 2f * MiniJson.GetNum(m, "medallion_half_w", 416f) * vmx;
            float medH = 2f * MiniJson.GetNum(m, "medallion_half_h", 208f) * vmy;
            for (int i = 0; medallions != null && i < medallions.Count && i < 5; i++)
            {
                var p = PsxPos(MiniJson.Get(medallions[i], "pos"), 2f);
                MakeQuad(glass, "medallion_" + i, quad,
                    Proj(p), ProjK(p.z) * new Vector2(medW, medH),
                    CutoutMat("medallion_" + i, "furniture/medallion_" + i + ".png",
                        false, null, Q_GLASS));
            }
            pedSpin = new Material[3];
            pedStop = new Material[3];
            float pedX0 = MiniJson.GetNum(m, "pedestal_x0", -384f);
            float pedXStep = MiniJson.GetNum(m, "pedestal_x_step", 384f);
            float pedY = MiniJson.GetNum(m, "pedestal_y", 480f);
            float pedW = 2f * MiniJson.GetNum(m, "pedestal_half_w", 560f) * vmx;
            float pedH = 2f * MiniJson.GetNum(m, "pedestal_half_h", 288f) * vmy;
            for (int r = 0; r < 3; r++)
            {
                pedSpin[r] = CutoutMat("pedestal_" + r + "_spin",
                    "furniture/pedestal_" + r + "_spin.png", false, null, Q_PEDESTAL);
                pedStop[r] = CutoutMat("pedestal_" + r + "_stop",
                    "furniture/pedestal_" + r + "_stop.png", false, null, Q_PEDESTAL);
                var p = new Vector3(pedX0 + r * pedXStep, -pedY, 801f);
                pedestalRenderers[r] = MakeQuad(glass, "pedestal_" + r, quad,
                    Proj(p), ProjK(p.z) * new Vector2(pedW, pedH), pedSpin[r]);
            }
            var marqueeBbs = MiniJson.AsList(MiniJson.Get(m, "marquee"));
            for (int i = 0; marqueeBbs != null && i < marqueeBbs.Count && i < 3; i++)
            {
                var bb = marqueeBbs[i];
                var p = PsxPos(MiniJson.Get(bb, "pos"), -1f);
                MakeQuad(glass, "marquee_bb_" + i, quad,
                    Proj(p),
                    ProjK(p.z) * new Vector2(
                        2f * MiniJson.GetNum(bb, "half_w", 100f) * vmx,
                        2f * MiniJson.GetNum(bb, "half_h", 100f) * vmy),
                    CutoutMat("marquee_" + i, "furniture/marquee_" + i + ".png",
                        false, null, Q_MARQUEE_BB));
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
                var a = PsxPos(MiniJson.Get(paylines[i], "a"), 4f);
                var b = PsxPos(MiniJson.Get(paylines[i], "b"), 4f);
                lr.SetPosition(0, Proj(a));
                lr.SetPosition(1, Proj(b));
                float lineW = 10f * worldUnit * ProjK((a.z + b.z) * 0.5f);
                lr.startWidth = lineW;
                lr.endWidth = lineW;
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
            // The whole dot grid inherits the projection through the anchor's
            // position and per-dot scale (dots are individually projected
            // model-space sprites in retail; +2 lifts them over the panel,
            // though the queue ladder is what actually orders the draw).
            float dotZ = -MiniJson.GetNum(m, "dot_z", -800f) + 2f;
            var anchorPos = new Vector3(dotX0, -dotY0, dotZ);
            float mk = ProjK(anchorPos.z);
            var marquee = Child(studio, "marquee", Proj(anchorPos));
            marquee.localScale = new Vector3(dotXStep * mk, dotYStep * mk, 1f);
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

        /// World-space TextMeshPro, not legacy TextMesh: the SDK exposes no
        /// TextMesh members to Udon, so a TextMesh HUD compiles in C# but
        /// fails the UdonSharp bind. TMP's default font asset auto-assigns
        /// on AddComponent (TMP essentials ship with the worlds SDK).
        static TMPro.TextMeshPro MakeText(Transform parent, string name, Vector3 localPos, float size)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            var tmp = go.AddComponent<TMPro.TextMeshPro>();
            tmp.alignment = TMPro.TextAlignmentOptions.Center;
            // TMP world text draws ~1 local unit of line height per 10 font
            // points; size*7 lands near the old TextMesh glyph height.
            tmp.fontSize = size * 7f;
            tmp.enableWordWrapping = false;
            tmp.color = new Color(1f, 0.9f, 0.6f);
            tmp.text = "";
            tmp.rectTransform.sizeDelta = new Vector2(size * 40f, size * 2f);
            var mr = go.GetComponent<MeshRenderer>();
            if (mr != null)
                mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            return tmp;
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
            if (parent == null)
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
            // Centre of the face, nudged just in front of it so the
            // composition covers the cabinet's own baked screen material.
            screenWidth = w;
            rigOffset = new Vector3(
                (min.x + max.x) * 0.5f,
                (min.y + max.y) * 0.5f,
                max.z + w * 0.01f);
            rigYaw = 0f;

            // The glb's own screen face carries whatever placeholder image
            // was baked into it, and the composition's relief quads don't
            // cover every pixel of it - black it out (on the scene instance
            // only; the imported glb asset is untouched, and a rebuild
            // reassigns the same material).
            var nodeMr = node.GetComponent<MeshRenderer>();
            if (nodeMr != null)
            {
                Undo.RecordObject(nodeMr, "Black out slot screen face");
                var black = BlackMat();
                var mats = new Material[nodeMr.sharedMaterials.Length];
                for (int i = 0; i < mats.Length; i++)
                    mats[i] = black;
                nodeMr.sharedMaterials = mats;
            }

            Debug.Log("[Legaia] screen snapped to node '" + name + "': " +
                w.ToString("0.###") + " x " + h.ToString("0.###") +
                " (cabinet units; composition is 4:3 " + w.ToString("0.###") + " x " +
                (w * 0.75f).ToString("0.###") + ") at " + rigOffset +
                "; the face's placeholder material was blacked out.");
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
            // Left-to-right AS THE PLAYER SEES THEM: the cabinet faces +z,
            // and a viewer looking back along -z sees cabinet +x on their
            // LEFT - so player-visual order is DESCENDING cabinet x. (The
            // same single mirror the screen composition carries; sorting
            // ascending wires the right-hand button to the left reel.)
            found.Sort((a, b) =>
                searchRoot.InverseTransformPoint(b.position).x
                    .CompareTo(searchRoot.InverseTransformPoint(a.position).x));
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
                    // just below the screen. Rig +x is the PLAYER'S LEFT
                    // (the rig faces +z), so pad 0 - reel 0, screen-left -
                    // sits at positive x.
                    go = GameObject.CreatePrimitive(PrimitiveType.Cube);
                    go.name = "slot_button_" + i;
                    go.transform.SetParent(rig.transform, false);
                    go.transform.localPosition = new Vector3(
                        0.17f - i * 0.17f,
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

        /// Per-cabinet suffix the pre-direct-screen builds keyed their feed
        /// assets by - kept so Build can delete a stale RenderTexture +
        /// screen material pair left by an older build of this cabinet.
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

        Material CutoutMat(string name, string relTex, bool repeat,
            Shader shader = null, int queue = 2450)
        {
            string texPath = artDir + "/" + relTex;
            var tex = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            if (tex == null)
                Debug.LogWarning("[Legaia] slot art texture missing: " + texPath);
            else
                ConfigureTexture(texPath, repeat);
            if (shader == null)
            {
                // The kit's Cull Off cutout: the mirrored composition
                // reverses winding, so the legacy back-culled cutout
                // renders nothing.
                shader = Shader.Find("LegaiaWorld/SlotCutout");
                if (shader == null)
                {
                    Debug.LogWarning("[Legaia] LegaiaWorld/SlotCutout shader " +
                        "not found (is Assets/LegaiaWorld/Shaders synced?) - " +
                        "'" + name + "' will be INVISIBLE (backface culled " +
                        "under the mirrored composition).");
                    shader = Shader.Find("Unlit/Transparent Cutout");
                }
            }
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
            // Overlay layer order under the z flatten (see the queue ladder).
            mat.renderQueue = queue;
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
            // The Cull Off cutout with a black tint (Unlit/Color would
            // backface-cull under the mirrored composition).
            var shader = Shader.Find("LegaiaWorld/SlotCutout");
            if (shader == null)
                shader = Shader.Find("Unlit/Color");
            var mat = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (mat == null)
            {
                mat = new Material(shader);
                AssetDatabase.CreateAsset(mat, path);
            }
            else if (mat.shader != shader)
            {
                mat.shader = shader;
            }
            mat.color = Color.black;
            mat.renderQueue = Q_BACKDROP;
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
