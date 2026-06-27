// BattleStateDecoder.cs
//
// The schema-driven battle-state decoder (PRD sec 5.4). Consumes the MIDI
// register stream emitted by midi_encoder.lua and reconstructs the latched
// BattleState, using the generated constants in Registers.cs so the two wire
// sides cannot drift. This is the start of the real decoder the diorama
// controller reads; for M0 it also renders a human-readable summary panel.
//
// Protocol (see register_schema.toml):
//   channel = address space (15 = meta, 0..2 = party, 3..7 = enemy slots)
//   cc      = register; value = 7-bit payload
//   wide registers are an (hi, lo) cc pair, 14-bit value, MSB first
//   commit (cc 0x7F) latches a channel's pending registers atomically
//
// LOCAL-only (MIDI fires only on the device-owning client). A separate sync
// relay (not this file) copies the decoded state into manual-synced variables
// for other players + late joiners (PRD M5).

using UdonSharp;
using UnityEngine;
using TMPro;
using LegaiaDiorama;

[UdonBehaviourSyncMode(BehaviourSyncMode.None)]
public class BattleStateDecoder : UdonSharpBehaviour
{
    [Tooltip("Optional panel for the decoded battle-state summary.")]
    [SerializeField] private TextMeshProUGUI _summary;

    private const int CHANNELS = 16;
    private const int REGISTERS = 128;
    private const int COMMIT = 127;          // cc 0x7F on every channel
    private const int SLOTS = 8;             // actor slots 0..7 (channel == slot)
    private const int PARTY_SLOTS = 3;       // 0..2 party, 3..7 enemy

    // Register file: pending receives writes; latched is applied on commit.
    private int[] _pending;   // [channel*128 + cc]
    private int[] _latched;

    // ---- decoded meta state (public for the diorama controller / sync relay) ----
    [System.NonSerialized] public int SchemaVersionSeen;
    [System.NonSerialized] public int GameMode;
    [System.NonSerialized] public int BattlePhase;
    [System.NonSerialized] public int Heartbeat;
    [System.NonSerialized] public int RegionId;
    [System.NonSerialized] public int RefreshMarker;

    // ---- decoded per-slot state ----
    [System.NonSerialized] public bool[] SlotPresent;
    [System.NonSerialized] public bool[] SlotDead;
    [System.NonSerialized] public bool[] SlotActing;
    [System.NonSerialized] public int[] SlotId;
    [System.NonSerialized] public int[] SlotHp;
    [System.NonSerialized] public int[] SlotMaxHp;
    [System.NonSerialized] public int[] SlotAction;
    [System.NonSerialized] public int[] SlotStatus;

    // staleness tracking (PRD R4): heartbeat that doesn't advance = frozen feed.
    private int _lastHeartbeat;
    private int _commitsSinceHeartbeatChange;

    private void Start()
    {
        _pending = new int[CHANNELS * REGISTERS];
        _latched = new int[CHANNELS * REGISTERS];
        SlotPresent = new bool[SLOTS];
        SlotDead = new bool[SLOTS];
        SlotActing = new bool[SLOTS];
        SlotId = new int[SLOTS];
        SlotHp = new int[SLOTS];
        SlotMaxHp = new int[SLOTS];
        SlotAction = new int[SLOTS];
        SlotStatus = new int[SLOTS];
        _lastHeartbeat = -1;
        Render();
    }

    public override void MidiControlChange(int channel, int number, int value)
    {
        if (channel < 0 || channel >= CHANNELS) { return; }
        if (number < 0 || number >= REGISTERS) { return; }

        if (number == COMMIT)
        {
            // Latch this channel's pending registers atomically, then decode it.
            int baseIdx = channel * REGISTERS;
            for (int cc = 0; cc < REGISTERS; cc++)
            {
                _latched[baseIdx + cc] = _pending[baseIdx + cc];
            }
            OnChannelCommitted(channel);
            Render();
            return;
        }
        _pending[channel * REGISTERS + number] = value;
    }

    private int Lat(int channel, int cc)
    {
        return _latched[channel * REGISTERS + cc];
    }

    // Reconstruct a 14-bit wide value from its (hi, lo) cc pair. MSB first.
    private int Wide(int channel, int hiCc, int loCc)
    {
        return (Lat(channel, hiCc) << 7) | Lat(channel, loCc);
    }

    private void OnChannelCommitted(int channel)
    {
        if (channel == Registers.MetaChannel)
        {
            SchemaVersionSeen = Lat(channel, Registers.MetaSchemaVersion);
            GameMode = Lat(channel, Registers.MetaGameMode);
            BattlePhase = Lat(channel, Registers.MetaBattlePhase);
            RefreshMarker = Lat(channel, Registers.MetaRefreshMarker);
            RegionId = Wide(channel, Registers.MetaRegionIdHi, Registers.MetaRegionIdLo);

            int hb = Lat(channel, Registers.MetaHeartbeat);
            if (hb != _lastHeartbeat) { _commitsSinceHeartbeatChange = 0; _lastHeartbeat = hb; }
            else { _commitsSinceHeartbeatChange++; }
            Heartbeat = hb;
            return;
        }

        if (channel >= 0 && channel < SLOTS)
        {
            int flags = Lat(channel, Registers.SlotFlags);
            SlotPresent[channel] = (flags & (1 << Registers.FlagPresent)) != 0;
            SlotDead[channel] = (flags & (1 << Registers.FlagDead)) != 0;
            SlotActing[channel] = (flags & (1 << Registers.FlagActing)) != 0;
            SlotId[channel] = Wide(channel, Registers.SlotIdHi, Registers.SlotIdLo);
            SlotHp[channel] = Wide(channel, Registers.SlotHpHi, Registers.SlotHpLo);
            SlotMaxHp[channel] = Wide(channel, Registers.SlotMaxhpHi, Registers.SlotMaxhpLo);
            SlotAction[channel] = Lat(channel, Registers.SlotActionId);
            SlotStatus[channel] = Lat(channel, Registers.SlotStatus);
        }
    }

    private string PhaseName(int p)
    {
        if (p == Registers.PhaseNone) { return "none"; }
        if (p == Registers.PhaseIntro) { return "intro"; }
        if (p == Registers.PhaseActive) { return "active"; }
        if (p == Registers.PhaseVictory) { return "victory"; }
        if (p == Registers.PhaseDefeat) { return "defeat"; }
        return "?" + p;
    }

    private void Render()
    {
        if (_summary == null) { return; }
        string s = "BattleState decoder\n";
        if (SchemaVersionSeen != 0 && SchemaVersionSeen != Registers.SchemaVersion)
        {
            s += "** SCHEMA MISMATCH: stream v" + SchemaVersionSeen
                + " vs decoder v" + Registers.SchemaVersion + " **\n";
        }
        bool stale = _commitsSinceHeartbeatChange > 4;
        s += "phase=" + PhaseName(BattlePhase)
            + "  mode=0x" + GameMode.ToString("X2")
            + "  region=" + RegionId
            + "  hb=" + Heartbeat + (stale ? " [STALE]" : " [LIVE]")
            + "  refresh=" + RefreshMarker + "\n";

        for (int slot = 0; slot < SLOTS; slot++)
        {
            if (!SlotPresent[slot]) { continue; }
            string role = (slot < PARTY_SLOTS) ? "P" : "E";
            s += role + slot
                + "  id=" + SlotId[slot]
                + "  hp=" + SlotHp[slot] + "/" + SlotMaxHp[slot]
                + "  act=" + SlotAction[slot]
                + (SlotDead[slot] ? " DEAD" : "")
                + (SlotActing[slot] ? " *acting*" : "")
                + (SlotStatus[slot] != 0 ? " st=" + SlotStatus[slot] : "")
                + "\n";
        }
        _summary.text = s;
    }
}
