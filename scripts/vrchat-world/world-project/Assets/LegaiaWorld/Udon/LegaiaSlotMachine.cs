// Udon behaviour for the casino slot-machine cabinet: a from-scratch port of
// the engine's rules kernel (`crates/engine-core/src/slot_machine.rs`, itself
// the confirmed-arithmetic port of the retail overlay PROT 0975 - see
// docs/subsystems/minigame-slot-machine.md). The confirmed pieces carried
// over verbatim:
//
// - the slot LCG (`x = x*5 + 1`, 16-bit halves folded) and the entry seed
//   0x6C0A2AF0;
// - the two 20-slot reel strips per reel (symbols `slot/2` probe +13, bonus
//   numerals `slot/2 + 0x10` probe +1), built in retail's interleaved draw
//   order, plus the display strip the win eval and renderer read;
// - the one-row-per-frame display refill 9 rows ahead of the payline - the
//   whole bonus-round "reels rotate onto numbers" mechanism;
// - the flat 3-coin bet (1 in feature modes 4..6), the net-take counter
//   (+6/+1 per spin, minus bonus payouts) and its feature-odds brackets;
// - the per-mode stop plan + landing search (mode 6 = depth 0, no target:
//   the bonus round steers nothing);
// - the five-payline all-equal evaluation, the per-symbol payout table, the
//   jackpot symbols 8/9 opening 1/3 bonus rounds, and the bonus product
//   payout `(v0-0xF)*(v1-0xF)*(v2-0xF)` = 1..1000 coins;
// - the reel *drum* itself (FUN_801d0fa8): 8 faces re-derived per frame at a
//   22.5-degree pitch (0x100 of a 0x1000 turn) from base angle 0x380+frac,
//   on the y=585 / z=512 ellipse, the payline row on face 4 and the strip
//   walking downward as the angle advances. The depth-cue shade lives in the
//   LegaiaWorld/SlotReelFace shader (per pixel - retail is per vertex);
// - the 70-coin entry balance (retail's dev-launch fallback - exactly the
//   "no casino coin bank" situation a VRChat world is in).
//
// VRChat adaptations: the BIOS-rand feature stream is a plain LCG (as in the
// engine port), balance is per-cabinet and refills to 70 when it runs dry
// (free play - there is no casino bank to cash out to), the cash-out submenu
// is dropped, and a payout left unclaimed auto-collects (`autoCollect`).
//
// Sync: outcomes (stop rows, wins, balance, phase) are owner-authoritative
// and synced; reel animation runs locally on every client from the same
// deterministic strips. Both RNG streams are synced alongside the outcomes,
// so a change of owner continues the SAME streams instead of forking them -
// and ownership only transfers while the machine is idle (one seat per
// session, as retail), which also closes the double-stop race.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaSlotMachine : UdonSharpBehaviour
    {
        // --- wiring (set by LegaiaSlotMachineBuilder) ------------------------

        [Header("Reels")]
        [Tooltip("3 reel pivots at the drum centres; faces are their children.")]
        public Transform[] reelPivots;

        [Tooltip("24 face renderers, reel*8+face - retail's 8 per-frame drum faces.")]
        public MeshRenderer[] reelFaces;

        [Tooltip("20 materials: 0..9 the reel symbols, 10..19 the bonus numerals 1..10.")]
        public Material[] valueMaterials;

        [Tooltip("The reel drum's y radius in model units (retail 0x249).")]
        public float reelYRadius = 585f;

        [Tooltip("The reel drum's z radius in model units (retail cos >> 3).")]
        public float reelZRadius = 512f;

        [Tooltip("Width of one reel face in model units (retail 0x100).")]
        public float reelFaceWidth = 256f;

        [Tooltip("Drum-centre transform the face shader's depth cue is baked from.")]
        public Transform reelShadeOrigin;

        [Header("Glass furniture")]
        [Tooltip("5 payline lamp quads down the right (index = payline).")]
        public MeshRenderer[] lampRenderers;
        public Material lampLitMaterial;
        public Material lampUnlitMaterial;

        [Tooltip("5 payline line renderers (index = payline).")]
        public LineRenderer[] paylineLines;

        [Tooltip("3 reel-stop pedestal quads under the reels.")]
        public MeshRenderer[] pedestalRenderers;
        public Material[] pedestalSpinMaterials;
        public Material[] pedestalStopMaterials;

        [Header("Dot-matrix marquee")]
        [Tooltip("3 tally number slots (bonus round: n x n x n).")]
        public MeshRenderer[] tallySlots;
        [Tooltip("2 multiplication-sign slots between the tally numbers.")]
        public MeshRenderer[] timesSlots;
        [Tooltip("3 bonus-rounds-owed pip slots.")]
        public MeshRenderer[] pipSlots;
        [Tooltip("4 payout caption digit slots.")]
        public MeshRenderer[] payoutDigitSlots;
        [Tooltip("The 'coin' word slot after the payout digits.")]
        public MeshRenderer coinSlot;
        [Tooltip("The attract-legend window (UV-scrolled).")]
        public MeshRenderer legendQuad;
        [Tooltip("21 materials over the dot-matrix message bank PNGs.")]
        public Material[] messageMaterials;
        [Tooltip("21 message widths, in dot columns (slot-machine.json).")]
        public float[] messageWidths;
        [Tooltip("Dots per second the attract legend scrolls.")]
        public float legendScrollSpeed = 20f;

        [Header("Screen feed (flat-screen mode)")]
        [Tooltip("Camera filming the hidden studio into the screen RenderTexture; null in behind-glass mode.")]
        public Camera screenCamera;

        [Tooltip("The visible screen quad - the camera pauses when no player is near it.")]
        public Transform screenPlane;

        [Tooltip("Metres from the screen beyond which the studio camera stops rendering (frozen last frame stays up).")]
        public float cameraActiveDistance = 14f;

        [Header("HUD")]
        // World-space TextMeshPro, NOT legacy TextMesh - the SDK ships no
        // ExternUnityEngineTextMesh wrapper module, so TextMesh members are
        // not exposed to Udon at all.
        public TMPro.TextMeshPro balanceText;
        public TMPro.TextMeshPro statusText;

        [Header("Game data (slot-machine.json)")]
        [Tooltip("Per-symbol line payout, index = symbol id 0..9.")]
        public int[] payoutTable;
        public int kickSymbol = 8;
        public int kickRounds = 1;
        public int punchSymbol = 9;
        public int punchRounds = 3;

        [Tooltip("Refill the balance to 70 when a spin can't be paid for (free play - no casino bank in VRChat).")]
        public bool freePlayRefill = true;

        [Tooltip("Collect an unclaimed payout automatically after the hold (VRChat convenience; retail waits for input).")]
        public bool autoCollect = true;

        // Marquee layout (dot columns; retail constants, builder overwrites
        // from the manifest).
        public int[] tallyNumberCols = { 0x00, 0x20, 0x40 };
        public int[] tallyTimesCols = { 0x10, 0x30 };
        public int[] payoutDigitCols = { 0x00, 0x0D, 0x1A, 0x27 };
        public int payoutCoinsCol = 0x34;
        public int[] roundPipCols = { 0x00, 0x20, 0x40 };
        public int msgNumberBase = 6;
        public int msgTimes = 0x11;
        public int msgPipOn = 0x12;
        public int msgPipOff = 0x13;
        public int msgCoins = 0x14;
        public int dotCols = 78;
        public int dotRows = 13;

        // The editor verify harness (LegaiaSlotTools) drives the rules methods
        // directly outside the Udon runtime; this mutes the VRC serialization
        // call it cannot service. Never set in a world.
        [System.NonSerialized] public bool suppressSerialization;

        // --- rules constants (docs/subsystems/minigame-slot-machine.md) ------

        const int STRIP_LEN = 20;
        const int REEL_COUNT = 3;
        const int REEL_WRAP = STRIP_LEN << 8;      // positions wrap mod 0x1400
        const int DISPLAY_REFRESH_LEAD = 9;        // rows ahead of the payline
        const int MIN_SPIN_BALANCE = 3;
        const int SPIN_COST_NORMAL = 3;
        const int SPIN_COST_FEATURE = 1;
        const int NET_TAKE_NORMAL = 6;
        const int NET_TAKE_FEATURE = 1;
        const int ENTRY_BALANCE = 70;              // the retail dev-launch fallback
        const int BALANCE_CAP = 9999999;
        const int SPIN_UP_FRAMES = 30;
        const int BONUS_SPIN_UP_FRAMES = 0x18;     // buys the strip-swap travel
        const int FEATURE_MODE_BONUS = 6;
        const int BONUS_VALUE_BASE = 0x10;
        const int PAYOUT_HOLD_FRAMES = 110;        // payout tally display before auto-collect

        const int PHASE_IDLE = 1;
        const int PHASE_SPINNING = 2;
        const int PHASE_STOPPING = 3;
        const int PHASE_PAYOUT = 4;

        // Retail's entry seed (FUN_801cec94 writes this literal every entry).
        // Typed int, not uint: the literal fits a positive int, and UdonSharp
        // does not support the `unchecked` cast a uint constant would need to
        // land in the synced int field.
        const int ENTRY_LCG_SEED = 0x6C0A2AF0;

        // Reel drum (FUN_801d0fa8): 8 faces per frame, each 0x100 of a 0x1000
        // turn (22.5 degrees), first face's top edge at 0x380 + frac. Face 4
        // carries the payline row (the face whose depth cue peaks), and the
        // strip walks DOWNWARD as the angle advances. The edge tables below
        // are indexed at 16-angle-unit granularity (256 entries per turn).
        const int FACE_COUNT = 8;
        const int FACE_ANGLE_BASE = 0x380;
        const int FACE_ANGLE_STEP = 0x100;
        const int PAYLINE_FACE = 4;
        const int EDGE_TABLE = 256;                // 0x1000 / 16

        // Five paylines x three per-reel row offsets from the payline row
        // (top / middle / bottom / two diagonals). Not `readonly` - UdonSharp
        // does not support the modifier on array fields.
        int[] PAYLINE_OFFSETS = { 1, 1, 1, 0, 0, 0, -1, -1, -1, -1, 0, 1, 1, 0, -1 };

        // --- synced state (owner-authoritative) ------------------------------

        [UdonSynced] int syncSeed;
        [UdonSynced] int syncRngState;   // slot LCG - synced so a new owner continues the stream
        [UdonSynced] int syncRandState;  // feature-roll stream, ditto
        [UdonSynced] int syncSpinSerial;
        [UdonSynced] int syncSpinFrames = SPIN_UP_FRAMES;
        [UdonSynced] int syncPhase = PHASE_IDLE;
        [UdonSynced] int syncFeatureMode;
        [UdonSynced] int syncBonusSpins;
        [UdonSynced] int syncBalance = ENTRY_BALANCE;
        [UdonSynced] int syncNetTake;
        [UdonSynced] int syncNormalTarget = 2;
        [UdonSynced] int syncJitter;     // rand%5 sub-row landing nudge (visual)
        [UdonSynced] int syncStopRow0 = -1;
        [UdonSynced] int syncStopRow1 = -1;
        [UdonSynced] int syncStopRow2 = -1;
        [UdonSynced] int syncWinLine = -1;
        [UdonSynced] int syncWinSymbol = -1;
        [UdonSynced] int syncPayout;
        [UdonSynced] bool syncBonusJustEnded;

        // --- local state -----------------------------------------------------

        // Flattened [reel*20+row]. Strip values: symbol ids 0..9, or bonus
        // numeral values 0x10..0x19 once a bonus round has rotated them in.
        int[] displayStrip = new int[REEL_COUNT * STRIP_LEN];
        int[] symbolStrip = new int[REEL_COUNT * STRIP_LEN];
        int[] bonusStrip = new int[REEL_COUNT * STRIP_LEN];
        int[] reelPos = new int[REEL_COUNT];   // fixed point, high byte = row
        int[] reelVel = new int[REEL_COUNT];
        int[] localStopRow = { -1, -1, -1 };
        int[] claimed = new int[REEL_COUNT];   // payline value + 1, 0 = unclaimed

        // Drum-face shadows: what each of the 24 face quads currently shows.
        int[] faceShownValue = new int[REEL_COUNT * FACE_COUNT];
        int[] lastDrawnPos = { -1, -1, -1 };

        // Edge tables over the 0x1000-unit turn at 16-unit steps, in the rig's
        // local frame (y up = -PSX y, z toward the player = -PSX z).
        float[] edgeY = new float[EDGE_TABLE];
        float[] edgeZ = new float[EDGE_TABLE];
        float[] faceLen = new float[EDGE_TABLE]; // edge i -> edge i+16 chord

        int builtSeed = int.MinValue;
        int localSpinSerial;
        int spinTimer;
        int payoutTimer;
        // Both RNG states are ints holding the u32 bit patterns: Udon does
        // not expose the unsigned operators (uint % is a compile error), and
        // wrapping int arithmetic is bit-identical for * + << ^, with masked
        // arithmetic shifts standing in for the logical ones.
        int rngState;    // slot LCG (strips + landings)
        int randState;   // feature-roll stream (BIOS-rand stand-in)
        bool isLocalOwner;
        float tickAccum;
        float legendOffset;
        Material legendMaterial;
        int shownLegendMsg = -1;
        int shownStateHash = -1;
        int cameraPoll;
        bool visualsActive = true;

        void Start()
        {
            isLocalOwner = Networking.IsOwner(gameObject);
            BuildEdgeTables();
            BakeShadeVectors();
            if (isLocalOwner && syncSeed == 0)
            {
                syncSeed = ENTRY_LCG_SEED;
                BuildStrips(syncSeed);
                Serialize();
            }
            else
            {
                if (syncSeed == 0)
                    syncSeed = ENTRY_LCG_SEED;
                BuildStrips(syncSeed);
            }
            if (legendQuad != null)
                legendMaterial = legendQuad.material; // instance for UV scroll
            ApplySyncedStops();
            ForceRedraw();
            UpdateHud();
        }

        void BuildEdgeTables()
        {
            float ry = reelYRadius;
            float rz = reelZRadius;
            for (int i = 0; i < EDGE_TABLE; i++)
            {
                float a = i * 16 * (Mathf.PI * 2f / 4096f);
                // PSX: y(a) = sin(a) * -ry, z(a) = cos(a) * rz; the rig frame
                // negates both, so the drum top is up and the payline face
                // (angle 0x800) lands at z = +rz, toward the player.
                edgeY[i] = Mathf.Sin(a) * ry;
                edgeZ[i] = -Mathf.Cos(a) * rz;
            }
            for (int i = 0; i < EDGE_TABLE; i++)
            {
                int j = (i + 16) % EDGE_TABLE;
                float dy = edgeY[i] - edgeY[j];
                float dz = edgeZ[i] - edgeZ[j];
                faceLen[i] = Mathf.Sqrt(dy * dy + dz * dz);
            }
        }

        /// The face shader reconstructs machine-space z from world space via
        /// two baked vectors; refresh them here so a rig dragged after the
        /// build self-corrects. (The builder bakes the same pair at build
        /// time for the edit-mode preview.)
        void BakeShadeVectors()
        {
            if (reelShadeOrigin == null || valueMaterials == null)
                return;
            float unit = reelShadeOrigin.lossyScale.z;
            if (unit < 1e-6f)
                return;
            Vector4 origin = reelShadeOrigin.position;
            Vector4 axis = reelShadeOrigin.forward * (-1f / unit);
            for (int i = 0; i < valueMaterials.Length; i++)
            {
                Material m = valueMaterials[i];
                if (m != null && m.HasProperty("_ShadeOrigin"))
                {
                    m.SetVector("_ShadeOrigin", origin);
                    m.SetVector("_ShadeAxis", axis);
                }
            }
        }

        // --- input (LegaiaSlotButton forwards Interact here) -----------------

        public void PressButton(int index)
        {
            if (!isLocalOwner)
            {
                // One seat per session: the machine only changes hands while
                // idle. This keeps both RNG streams and the stop inputs in one
                // player's hands from bet to payout (and closes the race where
                // a second player re-stops a reel the owner already stopped).
                if (syncPhase != PHASE_IDLE)
                    return;
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
                isLocalOwner = true;
            }

            if (syncPhase == PHASE_IDLE)
            {
                TrySpin();
            }
            else if (syncPhase == PHASE_STOPPING || (syncPhase == PHASE_SPINNING && spinTimer <= 0))
            {
                if (syncPhase == PHASE_SPINNING)
                    syncPhase = PHASE_STOPPING;
                StopReel(index);
            }
            else if (syncPhase == PHASE_PAYOUT)
            {
                Collect();
            }
            UpdateHud();
        }

        public override void OnOwnershipTransferred(VRCPlayerApi player)
        {
            isLocalOwner = player != null && player.isLocal;
        }

        // --- RNG (FUN_801d30cc + the engine's BiosRand stand-in) -------------

        // The slot LCG: x = x*5 + 1, 16-bit halves folded. Int arithmetic
        // wraps exactly like the retail u32; `(x >> 16) & 0xFFFF` is the
        // logical right shift Udon's int can express.
        int NextRng()
        {
            int x = rngState * 5 + 1;
            rngState = (x << 16) + ((x >> 16) & 0xFFFF);
            return rngState;
        }

        /// The next slot-LCG value reduced mod `m`, as the retail u32 would
        /// be: the value is `low31 + 2^31*highbit`, so fold `2^31 mod m` in
        /// when the sign bit is set (2^31 = 2*2^30 keeps every term in int).
        int NextRngMod(int m)
        {
            int x = NextRng();
            int r = (x & 0x7FFFFFFF) % m;
            if (x < 0)
            {
                int t = 1073741824 % m;
                r = (r + t + t) % m;
            }
            return r;
        }

        int NextRand15()
        {
            randState = randState * 1103515245 + 12345;
            return (randState >> 16) & 0x7FFF;
        }

        /// Owner-side flush: mirror both RNG streams into the synced fields so
        /// the next owner continues them, then serialize.
        void Serialize()
        {
            syncRngState = rngState;
            syncRandState = randState;
            if (suppressSerialization)
                return;
            RequestSerialization();
        }

        // --- strip build (FUN_801cf0d8 case 0) -------------------------------

        void BuildStrips(int seed)
        {
            builtSeed = seed;
            rngState = seed;
            randState = seed ^ 0x5A5A5A5A;
            for (int i = 0; i < symbolStrip.Length; i++)
            {
                symbolStrip[i] = -1;
                bonusStrip[i] = -1;
            }
            for (int reel = 0; reel < REEL_COUNT; reel++)
            {
                int b = reel * STRIP_LEN;
                // Retail builds BOTH strips per reel in one interleaved pass
                // off the same RNG stream - the order is what the strips are.
                for (int slot = 0; slot < STRIP_LEN; slot++)
                {
                    int pos = NextRngMod(STRIP_LEN);
                    while (symbolStrip[b + pos] != -1)
                        pos = (pos + 13) % STRIP_LEN;
                    symbolStrip[b + pos] = slot / 2;

                    pos = NextRngMod(STRIP_LEN);
                    while (bonusStrip[b + pos] != -1)
                        pos = (pos + 1) % STRIP_LEN;
                    bonusStrip[b + pos] = slot / 2 + BONUS_VALUE_BASE;
                }
            }
            for (int i = 0; i < displayStrip.Length; i++)
                displayStrip[i] = symbolStrip[i];
        }

        // --- spin (FUN_801cf0d8 states 1-2 + FUN_801d258c) -------------------

        void TrySpin()
        {
            if (syncBalance < MIN_SPIN_BALANCE)
            {
                if (!freePlayRefill)
                    return;
                syncBalance = ENTRY_BALANCE;
            }
            bool featureSpin = syncFeatureMode >= 4 && syncFeatureMode <= 6;
            syncBalance -= featureSpin ? SPIN_COST_FEATURE : SPIN_COST_NORMAL;
            syncNetTake += featureSpin ? NET_TAKE_FEATURE : NET_TAKE_NORMAL;
            FeatureRoll();

            // The long spin-up on a bonus spin AND the first spin after one:
            // the display strip needs to travel to swap sources (see the doc).
            syncSpinFrames = SPIN_UP_FRAMES;
            if (syncFeatureMode == FEATURE_MODE_BONUS || syncBonusJustEnded)
                syncSpinFrames += BONUS_SPIN_UP_FRAMES;
            syncBonusJustEnded = false;

            syncSpinSerial++;
            syncStopRow0 = -1;
            syncStopRow1 = -1;
            syncStopRow2 = -1;
            syncWinLine = -1;
            syncWinSymbol = -1;
            syncPayout = 0;
            syncPhase = PHASE_SPINNING;
            localSpinSerial = syncSpinSerial;
            StartSpinLocal(syncSpinFrames);
            Serialize();
        }

        // Per-spin roll: jitter (the sub-row landing nudge), the normal-mode
        // target symbol, and the net-take-bracketed feature entry.
        void FeatureRoll()
        {
            syncJitter = NextRand15() % 5;
            syncNormalTarget = NextRand15() % 6 + 2;
            if (syncFeatureMode != 0)
                return;
            int d1 = 0, d2 = 0;
            if (syncNetTake < 1000) { d1 = 700; d2 = 500; }
            else if (syncNetTake >= 1001 && syncNetTake <= 1999) { d1 = 350; d2 = 250; }
            else if (syncNetTake > 2000) { d1 = 175; d2 = 125; }
            int entered = 0;
            if (d1 != 0)
            {
                if (NextRand15() % d1 == 0)
                    entered = 1; // reach / jackpot tease (target symbol 9)
                // The mode-2 roll draws even when mode 1 already hit.
                if (NextRand15() % d2 == 0 && entered == 0)
                    entered = 2; // reach / bonus tease (target symbol 8)
            }
            if (NextRand15() % 600 == 0 && entered == 0)
                entered = 3; // hot mode
            if (entered != 0)
                syncFeatureMode = entered;
        }

        void StartSpinLocal(int frames)
        {
            for (int r = 0; r < REEL_COUNT; r++)
            {
                localStopRow[r] = -1;
                claimed[r] = 0;
            }
            reelVel[0] = 0x60;
            reelVel[1] = 0x70;
            reelVel[2] = 0x80;
            spinTimer = frames;
            // Reels start moving again - force the next visual pass.
            for (int r = 0; r < REEL_COUNT; r++)
                lastDrawnPos[r] = -1;
        }

        // --- stop (FUN_801d2114 / FUN_801d2440 / FUN_801d0554) ---------------

        void StopReel(int reel)
        {
            if (reel < 0 || reel >= REEL_COUNT || localStopRow[reel] != -1)
                return;

            // Guaranteed-hit mode drives later reels to the first landed
            // symbol; the bonus round does NOT (no target at all).
            int guarantee = -1;
            for (int r = 0; r < REEL_COUNT; r++)
                if (localStopRow[r] != -1)
                {
                    guarantee = displayStrip[localStopRow[r]]; // strips[0][row], as the engine port
                    break;
                }

            int depth;
            int target;
            int mode = syncFeatureMode;
            if (mode == 1) { depth = (NextRng() & 3) + 6; target = punchSymbol; }
            else if (mode == 2) { depth = (NextRng() & 3) + 6; target = kickSymbol; }
            else if (mode == FEATURE_MODE_BONUS) { depth = 0; target = -1; } // the free stop
            else if (mode == 4) { depth = STRIP_LEN; target = guarantee >= 0 ? guarantee : syncNormalTarget; }
            else { depth = NextRngMod(3) + 2; target = syncNormalTarget; }

            int fromRow = (reelPos[reel] >> 8) % STRIP_LEN;
            int row = LandRow(reel, fromRow, depth, target);
            SnapReel(reel, row);

            if (reel == 0) syncStopRow0 = row;
            else if (reel == 1) syncStopRow1 = row;
            else syncStopRow2 = row;

            int stoppedCount = 0;
            for (int r = 0; r < REEL_COUNT; r++)
                if (localStopRow[r] != -1)
                    stoppedCount++;
            if (stoppedCount == REEL_COUNT)
            {
                EvaluateSpin();
                syncPhase = PHASE_PAYOUT;
                payoutTimer = PAYOUT_HOLD_FRAMES;
            }
            Serialize();
        }

        int LandRow(int reel, int fromRow, int depth, int target)
        {
            // Retail guards the search with `0 < depth`: a zero depth searches
            // nothing at all - the bonus round's free stop.
            if (target >= 0 && depth > 0)
            {
                int limit = depth < STRIP_LEN ? depth : STRIP_LEN;
                for (int d = 0; d <= limit; d++)
                {
                    int row = (fromRow + d) % STRIP_LEN;
                    if (displayStrip[reel * STRIP_LEN + row] == target)
                        return row;
                }
            }
            return (fromRow + 1) % STRIP_LEN;
        }

        void SnapReel(int reel, int row)
        {
            reelPos[reel] = row << 8;
            reelVel[reel] = 0;
            localStopRow[reel] = row;
            lastDrawnPos[reel] = -1; // one final draw with the landing nudge
            // The claimed latch: payline value + 1 the frame the reel locks -
            // what the marquee's bonus tally prints (FUN_801d0554).
            claimed[reel] = displayStrip[reel * STRIP_LEN + row] + 1;
        }

        // --- win evaluation (FUN_801d13e8) ------------------------------------

        void EvaluateSpin()
        {
            bool bonusSpin = syncFeatureMode == FEATURE_MODE_BONUS && syncBonusSpins > 0;
            if (bonusSpin)
            {
                // Every bonus spin pays the product of the three payline
                // numbers - no all-equal gate, no payout table.
                int product = 1;
                bool allEqual = true;
                int first = displayStrip[localStopRow[0]];
                for (int r = 0; r < REEL_COUNT; r++)
                {
                    int v = displayStrip[r * STRIP_LEN + localStopRow[r]];
                    product *= v >= BONUS_VALUE_BASE ? v - 0xF : 1;
                    if (v != first)
                        allEqual = false;
                }
                syncNetTake -= product;
                syncBonusSpins--;
                if (syncBonusSpins <= 0)
                {
                    syncFeatureMode = 0;
                    // Latched so the NEXT spin runs long enough to rotate the
                    // symbols back onto the payline.
                    syncBonusJustEnded = true;
                }
                syncWinLine = 1; // retail forces the centre line / lamp
                syncWinSymbol = allEqual ? first : -1;
                syncPayout = product;
                return;
            }

            int bestLine = -1;
            int bestSymbol = -1;
            int bestValue = -1;
            for (int line = 0; line < 5; line++)
            {
                int a = LineSymbol(0, line);
                int b = LineSymbol(1, line);
                int c = LineSymbol(2, line);
                if (a == b && b == c)
                {
                    int value = a >= 0 && a < 10 && payoutTable != null && a < payoutTable.Length
                        ? payoutTable[a]
                        : 0;
                    if (value > bestValue)
                    {
                        bestLine = line;
                        bestSymbol = a;
                        bestValue = value;
                    }
                }
            }
            syncWinLine = bestLine;
            syncWinSymbol = bestSymbol;
            syncPayout = bestLine >= 0 ? bestValue : 0;
            if (bestSymbol == punchSymbol || bestSymbol == kickSymbol)
            {
                // The jackpot symbols open the bonus round: 3 rounds for the
                // red "punch", 1 for the blue "kick".
                syncFeatureMode = FEATURE_MODE_BONUS;
                syncBonusSpins = bestSymbol == punchSymbol ? punchRounds : kickRounds;
            }
            else if (bestLine >= 0 && syncFeatureMode != 0 && syncFeatureMode != 4)
            {
                // A resolved normal win clears a tease/hot feature.
                syncFeatureMode = 0;
            }
        }

        int LineSymbol(int reel, int line)
        {
            int off = PAYLINE_OFFSETS[line * REEL_COUNT + reel];
            int row = (localStopRow[reel] + off + STRIP_LEN) % STRIP_LEN;
            return displayStrip[reel * STRIP_LEN + row];
        }

        void Collect()
        {
            int credit = syncPayout;
            syncBalance += credit;
            if (syncBalance > BALANCE_CAP)
                syncBalance = BALANCE_CAP;
            syncPhase = PHASE_IDLE;
            Serialize();
        }

        // --- sync ------------------------------------------------------------

        public override void OnDeserialization()
        {
            if (syncSeed != builtSeed && syncSeed != 0)
                BuildStrips(syncSeed);
            // Continue the owner's streams, not our own fork: if this client
            // later takes the machine, its draws pick up where the previous
            // owner's left off.
            if (!isLocalOwner)
            {
                rngState = syncRngState;
                randState = syncRandState;
            }
            if (syncSpinSerial != localSpinSerial)
            {
                localSpinSerial = syncSpinSerial;
                StartSpinLocal(syncSpinFrames);
            }
            ApplySyncedStops();
            if (syncPhase == PHASE_PAYOUT && payoutTimer <= 0)
                payoutTimer = PAYOUT_HOLD_FRAMES;
            UpdateHud();
        }

        void ApplySyncedStops()
        {
            if (syncStopRow0 != -1 && localStopRow[0] == -1) SnapReel(0, syncStopRow0);
            if (syncStopRow1 != -1 && localStopRow[1] == -1) SnapReel(1, syncStopRow1);
            if (syncStopRow2 != -1 && localStopRow[2] == -1) SnapReel(2, syncStopRow2);
        }

        // --- per-frame -------------------------------------------------------

        void Update()
        {
            // Fixed 60 Hz rules tick, decoupled from the render rate. The
            // accumulator is clamped so a long hitch can't bank a burst of
            // catch-up ticks (or grow forever past the safety cap).
            tickAccum += Time.deltaTime;
            if (tickAccum > 0.1f)
                tickAccum = 0.1f;
            int safety = 4;
            while (tickAccum >= 1f / 60f && safety-- > 0)
            {
                tickAccum -= 1f / 60f;
                Tick();
            }

            // The studio camera is per-client cost: pause it (locally) when
            // this player is nowhere near the cabinet screen. The same gate
            // pauses all the visual work - what the camera doesn't film,
            // nobody sees.
            cameraPoll++;
            if (cameraPoll >= 30)
            {
                cameraPoll = 0;
                UpdateCameraCulling();
            }
            if (!visualsActive)
                return;

            ApplyReelVisuals();
            int hash = StateHash();
            if (hash != shownStateHash)
            {
                shownStateHash = hash;
                RefreshFurniture();
                RefreshMarquee();
            }
            ScrollLegend();
        }

        void UpdateCameraCulling()
        {
            bool active = true;
            var player = Networking.LocalPlayer;
            if (player != null && screenPlane != null)
                active = Vector3.Distance(player.GetPosition(), screenPlane.position)
                    < cameraActiveDistance;
            if (screenCamera != null && screenCamera.enabled != active)
                screenCamera.enabled = active;
            if (active && !visualsActive)
                ForceRedraw(); // catch up everything missed while paused
            visualsActive = active;
        }

        void ForceRedraw()
        {
            for (int i = 0; i < faceShownValue.Length; i++)
                faceShownValue[i] = -1;
            for (int r = 0; r < REEL_COUNT; r++)
                lastDrawnPos[r] = -1;
            shownStateHash = -1;
        }

        /// Everything RefreshFurniture / RefreshMarquee draw from, folded to
        /// one word so an unchanged frame costs one comparison.
        int StateHash()
        {
            int h = syncPhase;
            h = h * 31 + syncWinLine;
            h = h * 31 + syncPayout;
            h = h * 31 + syncFeatureMode;
            h = h * 31 + syncBonusSpins;
            for (int r = 0; r < REEL_COUNT; r++)
            {
                h = h * 31 + claimed[r];
                h = h * 31 + localStopRow[r];
            }
            return h;
        }

        void Tick()
        {
            for (int r = 0; r < REEL_COUNT; r++)
                if (localStopRow[r] == -1 && reelVel[r] != 0)
                    reelPos[r] = (reelPos[r] + reelVel[r]) % REEL_WRAP;

            if (spinTimer > 0)
            {
                spinTimer--;
                if (spinTimer <= 0 && isLocalOwner && syncPhase == PHASE_SPINNING)
                {
                    syncPhase = PHASE_STOPPING;
                    Serialize();
                    UpdateHud();
                }
            }

            if (syncPhase == PHASE_PAYOUT && autoCollect && payoutTimer > 0)
            {
                payoutTimer--;
                if (payoutTimer <= 0 && isLocalOwner)
                {
                    Collect();
                    UpdateHud();
                }
            }

            // The display-strip refill: one row per reel per frame, 9 rows
            // ahead of the payline, from whichever source strip the feature
            // mode names. This IS the bonus swap - numbers rotate in and out.
            bool bonus = syncFeatureMode == FEATURE_MODE_BONUS;
            for (int r = 0; r < REEL_COUNT; r++)
            {
                int row = ((reelPos[r] >> 8) + DISPLAY_REFRESH_LEAD) % STRIP_LEN;
                int i = r * STRIP_LEN + row;
                displayStrip[i] = bonus ? bonusStrip[i] : symbolStrip[i];
            }
        }

        // --- reel drum (FUN_801d0fa8 at face granularity) --------------------

        /// Re-derive the 8 drum faces per reel from the live position, as
        /// retail does per frame: face f's top edge sits at angle
        /// 0x380 + frac + f*0x100 on the y/z ellipse, and carries strip row
        /// (payline_row + 4 - f). A reel whose position hasn't changed since
        /// the last pass costs one comparison.
        void ApplyReelVisuals()
        {
            if (reelPivots == null || reelFaces == null || valueMaterials == null)
                return;
            for (int r = 0; r < REEL_COUNT && r < reelPivots.Length; r++)
            {
                // Stopped reels draw once more with the landing nudge - the
                // rand%5 sub-row offset retail leaves the reel sitting on.
                int drawPos = reelPos[r];
                if (localStopRow[r] != -1)
                    drawPos += syncJitter * 0x10;
                if (drawPos == lastDrawnPos[r])
                    continue;
                lastDrawnPos[r] = drawPos;

                int row0 = (drawPos >> 8) % STRIP_LEN;
                int frac = drawPos & 0xFF;
                int baseEdge = (FACE_ANGLE_BASE + frac) >> 4;
                for (int f = 0; f < FACE_COUNT; f++)
                {
                    int i = r * FACE_COUNT + f;
                    if (i >= reelFaces.Length || reelFaces[i] == null)
                        continue;
                    Transform face = reelFaces[i].transform;
                    int iT = (baseEdge + f * 16) % EDGE_TABLE;
                    int iB = (iT + 16) % EDGE_TABLE;
                    float yT = edgeY[iT];
                    float yB = edgeY[iB];
                    float zT = edgeZ[iT];
                    float zB = edgeZ[iB];
                    float dy = yT - yB;
                    float dz = zT - zB;
                    face.localPosition =
                        new Vector3(0f, (yT + yB) * 0.5f, (zT + zB) * 0.5f);
                    // Quad +Z = the outward chord normal, +Y = up the edge.
                    face.localRotation = Quaternion.LookRotation(
                        new Vector3(0f, -dz, dy), new Vector3(0f, dy, dz));
                    face.localScale = new Vector3(reelFaceWidth, faceLen[iT], 1f);

                    // The strip walks downward as the angle advances: the
                    // payline face (4) carries row0, faces above carry the
                    // rows after it.
                    int idx = (row0 + STRIP_LEN + PAYLINE_FACE - f) % STRIP_LEN;
                    int v = displayStrip[r * STRIP_LEN + idx];
                    if (v != faceShownValue[i])
                    {
                        faceShownValue[i] = v;
                        int mat = v >= BONUS_VALUE_BASE ? 10 + (v - BONUS_VALUE_BASE) : v;
                        if (mat >= 0 && mat < valueMaterials.Length)
                            reelFaces[i].sharedMaterial = valueMaterials[mat];
                    }
                }
            }
        }

        void RefreshFurniture()
        {
            bool showWin = syncPhase == PHASE_PAYOUT && syncWinLine >= 0;
            if (lampRenderers != null)
                for (int i = 0; i < lampRenderers.Length; i++)
                    if (lampRenderers[i] != null)
                        lampRenderers[i].sharedMaterial =
                            showWin && i == syncWinLine ? lampLitMaterial : lampUnlitMaterial;
            if (paylineLines != null)
                for (int i = 0; i < paylineLines.Length; i++)
                    if (paylineLines[i] != null)
                    {
                        // Idle neutral half-grey; the winning line goes bright
                        // (retail overwrites only the colour bytes).
                        Color c = showWin && i == syncWinLine
                            ? new Color(1f, 1f, 0.5f, 0.65f)
                            : new Color(0.5f, 0.5f, 0.5f, 0.35f);
                        paylineLines[i].startColor = c;
                        paylineLines[i].endColor = c;
                    }
            if (pedestalRenderers != null)
                for (int r = 0; r < pedestalRenderers.Length; r++)
                    if (pedestalRenderers[r] != null && pedestalSpinMaterials != null
                        && pedestalStopMaterials != null && r < pedestalSpinMaterials.Length
                        && r < pedestalStopMaterials.Length)
                        pedestalRenderers[r].sharedMaterial = localStopRow[r] != -1
                            ? pedestalStopMaterials[r]
                            : pedestalSpinMaterials[r];
        }

        // --- marquee (FUN_801cfff0 at placement granularity) -----------------

        void RefreshMarquee()
        {
            bool captionUp = syncPhase == PHASE_PAYOUT && syncPayout != 0;
            bool tallyModes = syncFeatureMode >= 4 && syncFeatureMode <= 6;
            bool tallyUp = !captionUp && tallyModes
                && (syncPhase == PHASE_STOPPING || syncPhase == PHASE_PAYOUT);
            bool pipsUp = !captionUp && tallyModes
                && (syncPhase == PHASE_IDLE || syncPhase == PHASE_SPINNING);
            bool legendUp = !captionUp && !tallyUp && !pipsUp;

            if (captionUp)
            {
                int n = syncPayout;
                int[] digits = new int[4];
                digits[0] = n / 1000;
                digits[1] = (n % 1000) / 100;
                digits[2] = (n % 100) / 10;
                digits[3] = n % 10;
                // Leading-zero suppression tests the WHOLE figure per place
                // (an interior zero is kept: 405 prints 4 0 5).
                int[] thresholds = { 1000, 100, 10, 0 };
                for (int i = 0; i < 4; i++)
                    ShowSlot(payoutDigitSlots, i, n >= thresholds[i],
                        msgNumberBase + digits[i], payoutDigitCols[i]);
                ShowSingleSlot(coinSlot, true, msgCoins, payoutCoinsCol);
            }
            else
            {
                for (int i = 0; i < 4; i++)
                    ShowSlot(payoutDigitSlots, i, false, 0, 0);
                ShowSingleSlot(coinSlot, false, 0, 0);
            }

            for (int i = 0; i < 3; i++)
            {
                // The tally prints the claimed number, or the "0" glyph while
                // a column is unclaimed.
                int number = claimed[i] < BONUS_VALUE_BASE ? 0 : claimed[i] - BONUS_VALUE_BASE;
                ShowSlot(tallySlots, i, tallyUp, msgNumberBase + number, tallyNumberCols[i]);
                bool lit = syncBonusSpins > i;
                ShowSlot(pipSlots, i, pipsUp, lit ? msgPipOn : msgPipOff, roundPipCols[i]);
            }
            for (int i = 0; i < 2; i++)
                ShowSlot(timesSlots, i, tallyUp, msgTimes, tallyTimesCols[i]);

            if (legendQuad != null && legendQuad.gameObject.activeSelf != legendUp)
                legendQuad.gameObject.SetActive(legendUp);
        }

        void ShowSlot(MeshRenderer[] slots, int index, bool on, int msg, int col)
        {
            if (slots == null || index >= slots.Length)
                return;
            ShowSingleSlot(slots[index], on, msg, col);
        }

        void ShowSingleSlot(MeshRenderer slot, bool on, int msg, int col)
        {
            if (slot == null)
                return;
            if (slot.gameObject.activeSelf != on)
                slot.gameObject.SetActive(on);
            if (!on || messageMaterials == null || msg < 0 || msg >= messageMaterials.Length)
                return;
            slot.sharedMaterial = messageMaterials[msg];
            float w = messageWidths != null && msg < messageWidths.Length ? messageWidths[msg] : 13f;
            // Slot local frame: 1 unit = 1 dot, origin at the matrix top-left.
            Transform t = slot.transform;
            t.localPosition = new Vector3(col + w * 0.5f, -dotRows * 0.5f, 0f);
            t.localScale = new Vector3(w, dotRows, 1f);
        }

        void ScrollLegend()
        {
            if (legendMaterial == null || legendQuad == null || !legendQuad.gameObject.activeSelf)
                return;
            if (messageMaterials != null && shownLegendMsg != 0 && messageMaterials.Length > 0)
            {
                // Attract legend is message 0; keep the instance's texture in
                // step so the scroll survives a material rebuild.
                legendMaterial.mainTexture = messageMaterials[0].mainTexture;
                shownLegendMsg = 0;
            }
            float w = messageWidths != null && messageWidths.Length > 0 ? messageWidths[0] : 78f;
            if (w <= 0f)
                return;
            legendOffset += Time.deltaTime * legendScrollSpeed / w;
            if (legendOffset > 1f)
                legendOffset -= 2f; // let the text run fully out before re-entering
            legendMaterial.mainTextureScale = new Vector2(dotCols / w, 1f);
            legendMaterial.mainTextureOffset = new Vector2(legendOffset, 0f);
        }

        // --- HUD -------------------------------------------------------------

        void UpdateHud()
        {
            if (balanceText != null)
                balanceText.text = "COIN " + syncBalance.ToString("00000");
            if (statusText == null)
                return;
            if (syncPhase == PHASE_IDLE)
            {
                if (syncFeatureMode == FEATURE_MODE_BONUS)
                    statusText.text = "BONUS GAME - PRESS TO SPIN (" + syncBonusSpins + " LEFT)";
                else if (syncBalance < MIN_SPIN_BALANCE && !freePlayRefill)
                    statusText.text = "OUT OF COINS";
                else
                    statusText.text = "INSERT 3 COINS - PRESS ANY BUTTON";
            }
            else if (syncPhase == PHASE_SPINNING)
                statusText.text = "GOOD LUCK...";
            else if (syncPhase == PHASE_STOPPING)
                statusText.text = "PRESS THE BUTTONS TO STOP THE REELS";
            else if (syncPhase == PHASE_PAYOUT)
                statusText.text = syncPayout > 0
                    ? "WIN " + syncPayout + " COIN" + (syncPayout == 1 ? "" : "S")
                    : "NO WIN";
        }
    }
}
