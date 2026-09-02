// Camp props: the pickup settings panel, and the toggleable torches +
// campfires near spawn. Everything here is built from primitives,
// generated materials and synthesized audio (LegaiaAudioGen) - no game
// data - and lives in its own top-level "Legaia_camp_props" container,
// deliberately OUTSIDE the mirrored scene root: world-space UI text
// under the root's X-flip would render mirror-written, and pickups
// under a negatively-scaled parent confuse physics.
//
// - Menu panel: hand-held board with world-space buttons (VRCUiShape)
//   that SendCustomEvent into LegaiaWorldMenu - local music mute,
//   synced day/night jumps (the realism pass wires the dayNight ref
//   when it builds the cycle).
// - Torches / campfires: LegaiaTorch pickups - hold + Use toggles the
//   flame (fire + smoke particles and a flickering point light - no
//   visible glow orb, the light itself is the effect) and a spatial
//   crackle loop.

using System.IO;
using UnityEditor;
using UnityEngine;
using UnityEngine.UI;

namespace LegaiaWorld
{
    internal static class LegaiaCampProps
    {
        internal const string CONTAINER = "Legaia_camp_props";

        internal static void Build(
            string genDir, string sceneName, Vector3 spawnW, AudioSource music)
        {
            var old = GameObject.Find(CONTAINER);
            if (old != null)
                Undo.DestroyObjectImmediate(old);
            var container = new GameObject(CONTAINER);
            Undo.RegisterCreatedObjectUndo(container, "Build Legaia camp props");

            Directory.CreateDirectory(genDir);
            var fireClip = LegaiaAudioGen.EnsureClip(
                genDir + "/fire_crackle.wav", LegaiaAudioGen.FireCrackle);
            var wood = EnsureMat(genDir, "camp_wood", "Standard",
                new Color(0.36f, 0.24f, 0.13f));
            var dark = EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            // Stale glow-orb material from earlier kit versions - the flames
            // no longer carry a visible glow sphere.
            AssetDatabase.DeleteAsset(genDir + "/camp_glow.mat");
            var flameMat = EnsureFlameMaterial(genDir);
            var smokeMat = EnsureSmokeMaterial(genDir);

            var pickupType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCPickup");
            var syncType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCObjectSync");

            BuildTorch(container, Ground(spawnW + new Vector3(1.8f, 0f, 1.4f)),
                fireClip, wood, dark, flameMat, smokeMat, pickupType, syncType, 1);
            BuildTorch(container, Ground(spawnW + new Vector3(-1.6f, 0f, 1.2f)),
                fireClip, wood, dark, flameMat, smokeMat, pickupType, syncType, 2);
            BuildCampfire(container, Ground(spawnW + new Vector3(2.6f, 0f, -2.1f)),
                fireClip, wood, flameMat, smokeMat, pickupType, syncType, 1);
            BuildCampfire(container, Ground(spawnW + new Vector3(-2.5f, 0f, 2.7f)),
                fireClip, wood, flameMat, smokeMat, pickupType, syncType, 2);
            BuildMenu(container, spawnW, sceneName, music, dark,
                pickupType, syncType);

            Debug.Log("[Legaia] camp props: settings panel, 2 torches and " +
                "2 campfires placed near spawn (hold + Use toggles a fire).");
        }

        /// Snap a point to the walkable ground under it (the world colliders
        /// exist by the time camp props build).
        internal static Vector3 Ground(Vector3 p)
        {
            if (Physics.Raycast(p + Vector3.up * 4f, Vector3.down,
                    out RaycastHit hit, 40f, ~0, QueryTriggerInteraction.Ignore))
                return hit.point;
            return p;
        }

        internal static Material EnsureMat(string genDir, string name, string shader, Color c)
        {
            string path = genDir + "/" + name + ".mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (m == null)
            {
                m = new Material(Shader.Find(shader));
                AssetDatabase.CreateAsset(m, path);
            }
            m.color = c;
            return m;
        }

        /// Additive particle material over a generated soft radial-glow
        /// sprite - the flame billboard.
        internal static Material EnsureFlameMaterial(string genDir)
        {
            string texPath = genDir + "/flame_soft.png";
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                const int S = 64;
                var tex = new Texture2D(S, S, TextureFormat.RGBA32, false);
                for (int y = 0; y < S; y++)
                    for (int x = 0; x < S; x++)
                    {
                        float d = Vector2.Distance(new Vector2(x, y),
                            new Vector2(S / 2f, S / 2f)) / (S / 2f);
                        float a = Mathf.Clamp01(1f - d);
                        a *= a;
                        tex.SetPixel(x, y, new Color(1f, 1f, 1f, a));
                    }
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
            }
            string matPath = genDir + "/flame_particle.mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (m == null)
            {
                m = new Material(Shader.Find("Legacy Shaders/Particles/Additive"));
                AssetDatabase.CreateAsset(m, matPath);
            }
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            return m;
        }

        /// Alpha-blended particle material over the same soft radial sprite -
        /// the smoke billboard (additive can't darken, so smoke needs its
        /// own blend mode).
        internal static Material EnsureSmokeMaterial(string genDir)
        {
            string matPath = genDir + "/smoke_particle.mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (m == null)
            {
                m = new Material(
                    Shader.Find("Legacy Shaders/Particles/Alpha Blended"));
                AssetDatabase.CreateAsset(m, matPath);
            }
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(
                genDir + "/flame_soft.png");
            return m;
        }

        /// The flame visual stack: fire particles + a smoke plume + a point
        /// light (LegaiaTorch flickers it), in an initially-inactive container
        /// LegaiaTorch toggles. No visible glow mesh - the light is the
        /// effect. `size` 1 = torch, ~1.8 = campfire.
        internal static GameObject BuildFlame(Transform parent, Vector3 localPos,
            float size, Material flameMat, Material smokeMat)
        {
            var flame = new GameObject("flame");
            flame.transform.SetParent(parent, false);
            flame.transform.localPosition = localPos;

            var ps = flame.AddComponent<ParticleSystem>();
            var main = ps.main;
            main.startLifetime = new ParticleSystem.MinMaxCurve(0.4f, 0.75f);
            main.startSpeed = new ParticleSystem.MinMaxCurve(0.45f * size, 0.8f * size);
            main.startSize = new ParticleSystem.MinMaxCurve(0.16f * size, 0.3f * size);
            main.startColor = new ParticleSystem.MinMaxGradient(
                new Color(1f, 0.75f, 0.3f), new Color(1f, 0.42f, 0.12f));
            main.simulationSpace = ParticleSystemSimulationSpace.Local;
            main.maxParticles = 64;
            var emission = ps.emission;
            emission.rateOverTime = 16f * size;
            var shape = ps.shape;
            shape.shapeType = ParticleSystemShapeType.Cone;
            shape.angle = 9f;
            shape.radius = 0.035f * size;
            var col = ps.colorOverLifetime;
            col.enabled = true;
            var grad = new Gradient();
            grad.SetKeys(
                new[]
                {
                    new GradientColorKey(new Color(1f, 0.9f, 0.55f), 0f),
                    new GradientColorKey(new Color(1f, 0.45f, 0.1f), 0.55f),
                    new GradientColorKey(new Color(0.55f, 0.12f, 0.03f), 1f),
                },
                new[]
                {
                    new GradientAlphaKey(0.9f, 0f),
                    new GradientAlphaKey(0.5f, 0.6f),
                    new GradientAlphaKey(0f, 1f),
                });
            col.color = new ParticleSystem.MinMaxGradient(grad);
            var sol = ps.sizeOverLifetime;
            sol.enabled = true;
            sol.size = new ParticleSystem.MinMaxCurve(1f,
                AnimationCurve.EaseInOut(0f, 1f, 1f, 0.25f));
            var psr = flame.GetComponent<ParticleSystemRenderer>();
            psr.sharedMaterial = flameMat;
            // VRChat recommends no camera roll on billboards: rolling with
            // head tilt breaks immersion in VR (the SDK flags it otherwise).
            psr.allowRoll = false;

            // A faint smoke plume rising off the flame tips: few particles,
            // slow, growing and fading as they climb.
            var smokeGo = new GameObject("smoke");
            smokeGo.transform.SetParent(flame.transform, false);
            smokeGo.transform.localPosition = new Vector3(0f, 0.25f * size, 0f);
            var sps = smokeGo.AddComponent<ParticleSystem>();
            var smain = sps.main;
            smain.startLifetime = new ParticleSystem.MinMaxCurve(1.4f, 2.4f);
            smain.startSpeed = new ParticleSystem.MinMaxCurve(0.25f * size, 0.45f * size);
            smain.startSize = new ParticleSystem.MinMaxCurve(0.14f * size, 0.22f * size);
            smain.startColor = new Color(0.28f, 0.26f, 0.24f, 0.30f);
            smain.simulationSpace = ParticleSystemSimulationSpace.World;
            smain.maxParticles = 32;
            var semission = sps.emission;
            semission.rateOverTime = 4.5f * size;
            var sshape = sps.shape;
            sshape.shapeType = ParticleSystemShapeType.Cone;
            sshape.angle = 14f;
            sshape.radius = 0.03f * size;
            var scol = sps.colorOverLifetime;
            scol.enabled = true;
            var sgrad = new Gradient();
            sgrad.SetKeys(
                new[]
                {
                    new GradientColorKey(new Color(0.30f, 0.28f, 0.26f), 0f),
                    new GradientColorKey(new Color(0.24f, 0.24f, 0.24f), 1f),
                },
                new[]
                {
                    new GradientAlphaKey(0f, 0f),
                    new GradientAlphaKey(0.8f, 0.25f),
                    new GradientAlphaKey(0f, 1f),
                });
            scol.color = new ParticleSystem.MinMaxGradient(sgrad);
            var ssol = sps.sizeOverLifetime;
            ssol.enabled = true;
            ssol.size = new ParticleSystem.MinMaxCurve(1f,
                AnimationCurve.EaseInOut(0f, 0.6f, 1f, 2.4f));
            var spsr = smokeGo.GetComponent<ParticleSystemRenderer>();
            spsr.sharedMaterial = smokeMat;
            spsr.allowRoll = false;

            var light = flame.AddComponent<Light>();
            light.type = LightType.Point;
            // A wide throw: a lit fire should carve a real pool out of the
            // night, not a puddle at its feet.
            light.range = 10.5f * size;
            // Base level LegaiaTorch's flicker wobbles around; a campfire
            // throws more light than a hand torch.
            light.intensity = 1.05f + 0.35f * size;
            light.color = new Color(1f, 0.62f, 0.3f);
            light.shadows = LightShadows.None;

            flame.SetActive(false);
            return flame;
        }

        /// Shared pickup plumbing: fitted box collider, spawn-kinematic body,
        /// VRC Pickup + Object Sync, crackle AudioSource, LegaiaTorch wiring.
        static void WireFirePickup(GameObject go, GameObject flame,
            AudioClip fireClip, float volume, float far,
            Vector3 colCenter, Vector3 colSize,
            System.Type pickupType, System.Type syncType)
        {
            var box = go.AddComponent<BoxCollider>();
            box.center = colCenter;
            box.size = colSize;
            var rb = go.AddComponent<Rigidbody>();
            rb.mass = 1f;
            rb.collisionDetectionMode = CollisionDetectionMode.ContinuousDynamic;
            rb.constraints = RigidbodyConstraints.FreezeRotation;
            rb.isKinematic = true; // LegaiaTorch frees it on first drop

            AudioSource crackle = null;
            if (fireClip != null)
            {
                crackle = go.AddComponent<AudioSource>();
                crackle.clip = fireClip;
                crackle.loop = true;
                crackle.playOnAwake = false;
                crackle.spatialBlend = 1f;
                crackle.volume = volume;
                crackle.maxDistance = far;
                LegaiaAudioGen.AddVrcSpatial(go, true, 8f, 0.6f, far);
            }

            if (pickupType != null)
            {
                var pickup = go.AddComponent(pickupType);
                // VR: Auto Hold keeps the torch in hand so the trigger is
                // free to mean Use (light / snuff) instead of drop.
                var autoHold = pickupType.GetField("AutoHold");
                if (autoHold != null && autoHold.FieldType.IsEnum)
                    autoHold.SetValue(pickup,
                        System.Enum.Parse(autoHold.FieldType, "Yes"));
                pickupType.GetField("UseText")?.SetValue(pickup, "Light / snuff");
                if (syncType != null)
                    go.AddComponent(syncType);
            }
            var torch = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaTorch");
            LegaiaWorldBuilder.SetUdonField(torch, "flame", flame);
            LegaiaWorldBuilder.SetUdonField(torch, "crackle", crackle);
            // The flame's point light, for the per-frame fire flicker.
            var fireLight = flame.GetComponent<Light>();
            LegaiaWorldBuilder.SetUdonField(torch, "fireLight", fireLight);
            LegaiaWorldBuilder.SetUdonField(torch, "fireIntensity",
                fireLight != null ? fireLight.intensity : 1.4f);
            LegaiaWorldBuilder.SyncUdonProxy(torch);
        }

        static void BuildTorch(GameObject container, Vector3 pos,
            AudioClip fireClip, Material wood, Material dark,
            Material flameMat, Material smokeMat,
            System.Type pickupType, System.Type syncType, int n)
        {
            var go = new GameObject("torch_" + n);
            go.transform.SetParent(container.transform, false);
            go.transform.position = pos;

            var handle = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
            handle.name = "handle";
            Object.DestroyImmediate(handle.GetComponent<Collider>());
            handle.transform.SetParent(go.transform, false);
            handle.transform.localPosition = new Vector3(0f, 0.28f, 0f);
            handle.transform.localScale = new Vector3(0.05f, 0.28f, 0.05f);
            handle.GetComponent<MeshRenderer>().sharedMaterial = wood;

            var head = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
            head.name = "head";
            Object.DestroyImmediate(head.GetComponent<Collider>());
            head.transform.SetParent(go.transform, false);
            head.transform.localPosition = new Vector3(0f, 0.58f, 0f);
            head.transform.localScale = new Vector3(0.075f, 0.045f, 0.075f);
            head.GetComponent<MeshRenderer>().sharedMaterial = dark;

            var flame = BuildFlame(go.transform, new Vector3(0f, 0.68f, 0f),
                1f, flameMat, smokeMat);
            WireFirePickup(go, flame, fireClip, 0.55f, 12f,
                new Vector3(0f, 0.33f, 0f), new Vector3(0.13f, 0.7f, 0.13f),
                pickupType, syncType);
        }

        static void BuildCampfire(GameObject container, Vector3 pos,
            AudioClip fireClip, Material wood,
            Material flameMat, Material smokeMat,
            System.Type pickupType, System.Type syncType, int n)
        {
            var go = new GameObject("campfire_" + n);
            go.transform.SetParent(container.transform, false);
            go.transform.position = pos;

            for (int i = 0; i < 3; i++)
            {
                var log = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
                log.name = "log_" + i;
                Object.DestroyImmediate(log.GetComponent<Collider>());
                log.transform.SetParent(go.transform, false);
                log.transform.localPosition = new Vector3(0f, 0.055f + i * 0.02f, 0f);
                log.transform.localRotation = Quaternion.Euler(90f, i * 60f, 0f);
                log.transform.localScale = new Vector3(0.055f, 0.24f, 0.055f);
                log.GetComponent<MeshRenderer>().sharedMaterial = wood;
            }

            var flame = BuildFlame(go.transform, new Vector3(0f, 0.12f, 0f),
                1.8f, flameMat, smokeMat);
            WireFirePickup(go, flame, fireClip, 0.8f, 18f,
                new Vector3(0f, 0.14f, 0f), new Vector3(0.55f, 0.3f, 0.55f),
                pickupType, syncType);
        }

        /// A planted, non-pickup stake torch for the realism pass's night
        /// torches: taller than the camp torches, flame active from the
        /// start (its container is what LegaiaDayNight toggles), point
        /// light flickered by LegaiaFlicker, low crackle that plays
        /// whenever the torch is enabled. No collider - these stand beside
        /// doorways and must never block a walk path.
        internal static void BuildNightTorch(Transform parent, Vector3 pos,
            AudioClip fireClip, Material wood, Material dark,
            Material flameMat, Material smokeMat, int n)
        {
            var go = new GameObject("night_torch_" + n);
            go.transform.SetParent(parent, false);
            go.transform.position = pos;

            var stake = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
            stake.name = "stake";
            Object.DestroyImmediate(stake.GetComponent<Collider>());
            stake.transform.SetParent(go.transform, false);
            stake.transform.localPosition = new Vector3(0f, 0.55f, 0f);
            stake.transform.localScale = new Vector3(0.055f, 0.55f, 0.055f);
            stake.GetComponent<MeshRenderer>().sharedMaterial = wood;

            var head = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
            head.name = "head";
            Object.DestroyImmediate(head.GetComponent<Collider>());
            head.transform.SetParent(go.transform, false);
            head.transform.localPosition = new Vector3(0f, 1.13f, 0f);
            head.transform.localScale = new Vector3(0.085f, 0.05f, 0.085f);
            head.GetComponent<MeshRenderer>().sharedMaterial = dark;

            var flame = BuildFlame(go.transform, new Vector3(0f, 1.24f, 0f),
                1f, flameMat, smokeMat);
            flame.SetActive(true); // burns whenever the night container is on

            var light = flame.GetComponent<Light>();
            var flick = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaFlicker");
            LegaiaWorldBuilder.SetUdonField(flick, "fireLight", light);
            LegaiaWorldBuilder.SetUdonField(flick, "fireIntensity",
                light != null ? light.intensity : 1.4f);
            LegaiaWorldBuilder.SyncUdonProxy(flick);

            if (fireClip != null)
            {
                var crackle = go.AddComponent<AudioSource>();
                crackle.clip = fireClip;
                crackle.loop = true;
                // playOnAwake restarts the loop each time LegaiaDayNight
                // re-enables the night container.
                crackle.playOnAwake = true;
                crackle.spatialBlend = 1f;
                crackle.volume = 0.4f;
                crackle.maxDistance = 10f;
                LegaiaAudioGen.AddVrcSpatial(go, true, 6f, 0.6f, 10f);
            }
        }

        // --- Settings panel --------------------------------------------------

        static void BuildMenu(GameObject container, Vector3 spawnW,
            string sceneName, AudioSource music, Material dark,
            System.Type pickupType, System.Type syncType)
        {
            Vector3 basePos = Ground(spawnW + new Vector3(0.9f, 0f, 0.9f));
            var go = new GameObject("LegaiaMenu");
            go.transform.SetParent(container.transform, false);
            go.transform.position = basePos + Vector3.up * 1.15f;
            // Board faces the spawn point: UI is visible from the canvas's
            // -Z side, so forward points AWAY from the viewer.
            Vector3 away = go.transform.position - (spawnW + Vector3.up * 1.5f);
            away.y = 0f;
            if (away.sqrMagnitude > 1e-6f)
                go.transform.rotation = Quaternion.LookRotation(away.normalized);

            var board = GameObject.CreatePrimitive(PrimitiveType.Cube);
            board.name = "board";
            Object.DestroyImmediate(board.GetComponent<Collider>());
            board.transform.SetParent(go.transform, false);
            board.transform.localScale = new Vector3(0.46f, 0.36f, 0.02f);
            board.GetComponent<MeshRenderer>().sharedMaterial = dark;

            // Grab HANDLE below the board. The pickup's collider must stay
            // out of the button area: VRChat's pointer raycasts colliders
            // and the closest hit wins, so a grab collider in front of the
            // canvas's VRCUiShape collider turns every button press into a
            // pickup grab and the UI never receives the click (a full-board
            // collider is exactly the bug that made the panel dead).
            var handle = GameObject.CreatePrimitive(PrimitiveType.Cube);
            handle.name = "handle";
            Object.DestroyImmediate(handle.GetComponent<Collider>());
            handle.transform.SetParent(go.transform, false);
            handle.transform.localPosition = new Vector3(0f, -0.235f, 0f);
            handle.transform.localScale = new Vector3(0.3f, 0.07f, 0.03f);
            handle.GetComponent<MeshRenderer>().sharedMaterial = dark;

            var box = go.AddComponent<BoxCollider>();
            box.center = new Vector3(0f, -0.235f, 0f);
            box.size = new Vector3(0.32f, 0.09f, 0.06f);
            var rb = go.AddComponent<Rigidbody>();
            rb.isKinematic = true; // the panel floats where it is left
            if (pickupType != null)
            {
                go.AddComponent(pickupType);
                if (syncType != null)
                    go.AddComponent(syncType);
            }

            var menu = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaWorldMenu");
            var backing = BackingUdon(menu);

            // World-space canvas, 7 mm in front of the board face.
            var canvasGo = new GameObject("canvas");
            canvasGo.transform.SetParent(go.transform, false);
            canvasGo.transform.localPosition = new Vector3(0f, 0f, -0.017f);
            canvasGo.transform.localScale = Vector3.one * 0.001f;
            var canvas = canvasGo.AddComponent<Canvas>();
            canvas.renderMode = RenderMode.WorldSpace;
            var rt = canvasGo.GetComponent<RectTransform>();
            rt.sizeDelta = new Vector2(420f, 340f);
            canvasGo.AddComponent<GraphicRaycaster>();
            var shapeType =
                LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCUiShape")
                ?? LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_UiShape");
            if (shapeType != null)
                canvasGo.AddComponent(shapeType);
            var cbox = canvasGo.AddComponent<BoxCollider>();
            cbox.size = new Vector3(420f, 340f, 10f);

            var font = MenuFont();
            MakeText(canvasGo.transform, font, sceneName, 24, new Vector2(0f, 130f),
                new Vector2(380f, 44f), new Color(1f, 0.9f, 0.7f));
            Text musicLabel = MakeButton(canvasGo.transform, font, "Music: On",
                new Vector2(0f, 55f), backing, "ToggleMusic");
            MakeButton(canvasGo.transform, font, "Daytime",
                new Vector2(0f, -35f), backing, "SetDay");
            MakeButton(canvasGo.transform, font, "Nighttime",
                new Vector2(0f, -125f), backing, "SetNight");

            LegaiaWorldBuilder.SetUdonField(menu, "music", music);
            LegaiaWorldBuilder.SetUdonField(menu, "musicLabel", musicLabel);
            // dayNight is wired by the realism pass when it builds the cycle.
            LegaiaWorldBuilder.SyncUdonProxy(menu);
        }

        /// The backing UdonBehaviour of a U# proxy - the component whose
        /// SendCustomEvent the UI buttons must target (clicking the proxy
        /// would do nothing in-world).
        static Component BackingUdon(Component proxy)
        {
            if (proxy == null)
                return null;
            var util = LegaiaWorldBuilder.FindType(
                "UdonSharpEditor.UdonSharpEditorUtility");
            var mi = util?.GetMethod("GetBackingUdonBehaviour");
            if (mi == null)
                return null;
            try
            {
                return mi.Invoke(null, new object[] { proxy }) as Component;
            }
            catch (System.Exception e)
            {
                Debug.LogWarning("[Legaia] GetBackingUdonBehaviour failed: " +
                    (e.InnerException ?? e).Message);
                return null;
            }
        }

        static Font MenuFont()
        {
            var f = Resources.GetBuiltinResource<Font>("LegacyRuntime.ttf");
            if (f == null)
                f = Resources.GetBuiltinResource<Font>("Arial.ttf");
            return f;
        }

        static Text MakeText(Transform parent, Font font, string label,
            int size, Vector2 pos, Vector2 dims, Color color)
        {
            var go = new GameObject("text_" + label);
            go.transform.SetParent(parent, false);
            var rt = go.AddComponent<RectTransform>();
            rt.anchoredPosition = pos;
            rt.sizeDelta = dims;
            var text = go.AddComponent<Text>();
            text.font = font;
            text.fontSize = size;
            text.text = label;
            text.color = color;
            text.alignment = TextAnchor.MiddleCenter;
            return text;
        }

        /// A UI button whose click sends `eventName` into the backing
        /// UdonBehaviour (the standard VRChat UI-to-Udon wire, recorded as a
        /// persistent listener so it survives into the client build).
        static Text MakeButton(Transform parent, Font font, string label,
            Vector2 pos, Component backing, string eventName)
        {
            var go = new GameObject("btn_" + eventName);
            go.transform.SetParent(parent, false);
            var rt = go.AddComponent<RectTransform>();
            rt.anchoredPosition = pos;
            rt.sizeDelta = new Vector2(340f, 72f);
            var img = go.AddComponent<Image>();
            img.color = new Color(0.28f, 0.25f, 0.2f, 0.95f);
            var btn = go.AddComponent<Button>();
            var colors = btn.colors;
            colors.highlightedColor = new Color(0.45f, 0.4f, 0.3f);
            colors.pressedColor = new Color(0.6f, 0.5f, 0.32f);
            btn.colors = colors;
            var text = MakeText(go.transform, font, label, 26, Vector2.zero,
                new Vector2(330f, 64f), new Color(0.95f, 0.92f, 0.85f));
            if (backing != null)
            {
                var action = (UnityEngine.Events.UnityAction<string>)
                    System.Delegate.CreateDelegate(
                        typeof(UnityEngine.Events.UnityAction<string>),
                        backing, "SendCustomEvent");
                UnityEditor.Events.UnityEventTools.AddStringPersistentListener(
                    btn.onClick, action, eventName);
            }
            return text;
        }
    }
}
