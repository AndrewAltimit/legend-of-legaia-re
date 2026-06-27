// MidiDebugMonitor.cs
//
// PRD milestone M0 -- the bare test-world MIDI monitor. Prints every incoming
// MidiControlChange event to a TextMeshPro panel (and the Udon log), with NO
// schema dependency. This is the gate that proves the last unverified hop:
// virtual MIDI port -> Wine/Proton -> VRChat actually delivers events to my
// client. (MIDI input is LOCAL-only: events fire only on the client that
// launched with --midi=<deviceName>, so this is a no-sync, local behaviour.)
//
// Setup (see world-project/README.md): add a UI Canvas + TextMeshProUGUI, put
// this component on a GameObject, assign `_log` to the text, launch VRChat with
// --midi="<port>", and drive the port (the live relay, or a desktop MIDI
// sender). Watch the panel.
//
// This file ships in Assets/LegaiaDiorama/ alongside BattleStateDecoder.cs and
// the generated Registers.cs; copy that folder into a VCC-created world project.

using UdonSharp;
using UnityEngine;
using TMPro;

[UdonBehaviourSyncMode(BehaviourSyncMode.None)]
public class MidiDebugMonitor : UdonSharpBehaviour
{
    [Tooltip("Panel that shows the most recent MIDI events.")]
    [SerializeField] private TextMeshProUGUI _log;

    [Tooltip("How many recent events to keep on screen.")]
    [SerializeField] private int _maxLines = 24;

    // Ring buffer of recent event lines.
    private string[] _lines;
    private int _head;      // next write index
    private int _count;     // lines currently stored
    private int _total;     // lifetime event count

    private void Start()
    {
        if (_maxLines < 1) { _maxLines = 1; }
        _lines = new string[_maxLines];
        _head = 0;
        _count = 0;
        _total = 0;
        Render();
    }

    // VRChat delivers each Control-Change here on the client owning the MIDI
    // device. channel 0..15, number (cc) 0..127, value 0..127.
    public override void MidiControlChange(int channel, int number, int value)
    {
        _total++;
        string line = "#" + _total + "  ch " + channel
            + "  cc 0x" + number.ToString("X2") + " (" + number + ")"
            + "  = " + value;
        Debug.Log("[MidiDebugMonitor] " + line);

        _lines[_head] = line;
        _head = (_head + 1) % _maxLines;
        if (_count < _maxLines) { _count++; }
        Render();
    }

    // Note (PRD sec 4): VRChat exposes MidiNoteOn / MidiNoteOff / MidiControlChange
    // only. The battle-diorama protocol uses Control-Change exclusively, but
    // these are included so the monitor shows ALL incoming MIDI during M0.
    public override void MidiNoteOn(int channel, int number, int velocity)
    {
        _total++;
        Append("#" + _total + "  ch " + channel + "  NOTE-ON " + number + " vel " + velocity);
    }

    public override void MidiNoteOff(int channel, int number, int velocity)
    {
        _total++;
        Append("#" + _total + "  ch " + channel + "  NOTE-OFF " + number);
    }

    private void Append(string line)
    {
        Debug.Log("[MidiDebugMonitor] " + line);
        _lines[_head] = line;
        _head = (_head + 1) % _maxLines;
        if (_count < _maxLines) { _count++; }
        Render();
    }

    private void Render()
    {
        if (_log == null) { return; }
        string outp = "MIDI monitor -- events received: " + _total + "\n";
        if (_count == 0)
        {
            outp += "(waiting for MIDI... launch with --midi=\"Virtual Raw MIDI\")";
        }
        else
        {
            // Oldest -> newest. Oldest line index when the buffer is full.
            int start = (_count < _maxLines) ? 0 : _head;
            for (int i = 0; i < _count; i++)
            {
                int idx = (start + i) % _maxLines;
                outp += _lines[idx] + "\n";
            }
        }
        _log.text = outp;
    }
}
