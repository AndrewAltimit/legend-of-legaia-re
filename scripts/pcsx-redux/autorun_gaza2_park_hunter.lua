-- autorun_gaza2_park_hunter.lua
--
-- Human-driven park hunting at full dynarec speed.
--
-- Every prior Gaza 2 probe in this family needed the interpreter because it
-- armed breakpoints. This one arms NONE: it is a pure per-vsync poller, so it
-- runs under the recompiler at whatever speed the emulator manages (3-4x with
-- the frame limiter loosened), while a human plays, savestates, reloads and
-- retries. The division of labour: this probe only has to CATCH the park and
-- freeze the moment; attribution then happens offline by replaying the
-- human's savestate under the interpreter probes.
--
-- What it watches (poll-only, all read every vsync):
--   * game mode 0x8007B83C     - battle (0x15) gates everything
--   * ctx+7 / ctx+0x6D8 / ctx+0x276 - the 0x51 exit gate's operands
--   * per party slot: live HP +0x14C, displayed HP +0x172, accumulator +0x10
--
-- Detections (both write a wedge file and, by default, PAUSE the emulator so
-- the frozen moment can be savestated at leisure):
--   PARK    ANY ctx+7 value held unchanged for LEGAIA_PARK_N consecutive
--           vsyncs while in battle (default 1500; the longest healthy dwell
--           measured on this save is state 0x36's ~1227). Band-agnostic on
--           purpose: the first live-caught park sat in 0x19 (attack
--           approach), not the 0x51 HP-settle gate this family started from.
--           States 0x00 (Begin/Run prompt) and 0xFF (round boundary) are
--           exempt - both legitimately wait on the player.
--   DESYNC  a party slot absorbing (+0x14C != +0x172 with +0x10 == 0, actor
--           alive) for LEGAIA_ABSORB_N consecutive vsyncs (default 900).
--           Phased mid-action crediting produces this shape transiently -
--           measured well under 200 vsyncs - so a long survivor is a real
--           desync even before anything parks on it.
--
-- After a pause: make a SAVESTATE of the frozen moment (that file is the
-- deliverable), then resume from the GUI (Emulation > Resume) and keep
-- playing - the detector re-arms only after the condition clears.
--
-- Outputs (captures/gaza2_park_hunter/<ts>/):
--   hunt.csv     change-triggered + heartbeat rows:
--                vsync,scene,ctx7,c6d8,c276,seat0 hp/bar/acc,seat1 ...,note
--   wedge_N.txt  full dump per detection
--
-- Knobs (env):
--   LEGAIA_PARK_N     park dwell threshold in vsyncs (default 600)
--   LEGAIA_ABSORB_N   surviving-desync threshold in vsyncs (default 900)
--   LEGAIA_PAUSE      1 = pause the emulator on detection (default 1)
--   LEGAIA_HEARTBEAT  unconditional row cadence while in battle (default 64)
--
-- Launch (dynarec ON, debugger OFF, YOUR config + memcards + savestates):
--   LEGAIA_NO_SSTATE=1 bash scripts/pcsx-redux/run_probe.sh \
--     --fast --no-isolate-config \
--     --lua scripts/pcsx-redux/autorun_gaza2_park_hunter.lua
-- then just play. Close the emulator when done; the CSV survives.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local PARK_N    = probe.getenv_num("LEGAIA_PARK_N", 1500)
local ABSORB_N  = probe.getenv_num("LEGAIA_ABSORB_N", 900)
local PAUSE     = probe.getenv_num("LEGAIA_PAUSE", 1) ~= 0
local HEARTBEAT = probe.getenv_num("LEGAIA_HEARTBEAT", 64)

local MODE_VA    = 0x8007B83C
local SCENE_VA   = 0x8007050C
local CTX_PTR    = 0x8007BD24
local ACTORS     = 0x801C9370
local CAM_YAW    = 0x8007B792

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function i32(a) local v = u32(a); return v >= 0x80000000 and v - 0x100000000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local function scene_name()
    local out = {}
    for k = 0, 7 do
        local b = u8(SCENE_VA + k)
        if b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local csv = probe.csv_open(probe.out_path("hunt.csv"),
    "vsync,scene,ctx7,c6d8,c276,hp0,bar0,acc0,hp1,bar1,acc1,hp2,bar2,acc2,note")

local vsync = 0
local last_sig = ""
local park_ctx7, park_since = nil, 0
local absorb_since = { [0] = 0, [1] = 0, [2] = 0 }
local wedge_n = 0
local triggered = false   -- latched until the condition clears

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

local function write_wedge(kind, detail)
    wedge_n = wedge_n + 1
    local lines = {
        string.format("=== gaza2 park hunter: %s at vsync %d ===", kind, vsync),
        string.format("scene=%s  mode=0x%02X  cam_yaw=0x%04X",
            scene_name(), u8(MODE_VA), u16(CAM_YAW)),
        detail,
        "",
        "NOW: make a savestate of this frozen moment (the deliverable),",
        "then resume from the GUI and keep playing.",
    }
    local c = u32(CTX_PTR)
    if in_ram(c) then
        lines[#lines + 1] = string.format(
            "ctx=0x%08X ctx7=0x%02X c6d8=%d c276=%d acting=%d",
            c, u8(c + 7), i16(c + 0x6D8), u8(c + 0x276), u8(c + 0x13))
        local aa = actor_of(u8(c + 0x13))
        if aa ~= 0 then
            lines[#lines + 1] = string.format("acting +0x1DD=%d", u8(aa + 0x1DD))
        end
        for s = 0, 7 do
            local a = actor_of(s)
            if a ~= 0 then
                lines[#lines + 1] = string.format(
                    "  slot %d: hp=%d/%d bar=%d acc=%d",
                    s, u16(a + 0x14C), u16(a + 0x14E), u16(a + 0x172), i32(a + 0x10))
            end
        end
    end
    local body = table.concat(lines, "\n")
    probe.write_snapshot(probe.out_path(string.format("wedge_%02d.txt", wedge_n)), body)
    for _, l in ipairs(lines) do PCSX.log("[hunter] " .. l) end
    if PAUSE then PCSX.pauseEmulator() end
end

local function on_vsync()
    vsync = vsync + 1
    if u8(MODE_VA) ~= 0x15 then
        park_since = 0
        absorb_since[0], absorb_since[1], absorb_since[2] = 0, 0, 0
        triggered = false
        return
    end
    local c = u32(CTX_PTR)
    if not in_ram(c) then return end

    local ctx7 = u8(c + 7)
    local c6d8 = i16(c + 0x6D8)
    local c276 = u8(c + 0x276)

    -- Park detector: ANY battle-action state held past the longest healthy
    -- dwell. 0x00 (the Begin/Run prompt) and 0xFF (round boundary) wait on
    -- the player and are exempt.
    if ctx7 == park_ctx7 and ctx7 ~= 0x00 and ctx7 ~= 0xFF then
        park_since = park_since + 1
    else
        park_ctx7, park_since = ctx7, 0
    end

    local row = { vsync, scene_name(), string.format("0x%02X", ctx7), c6d8, c276 }
    local note = ""
    local any_absorb = false
    for s = 0, 2 do
        local a = actor_of(s)
        local hp, bar, acc = -1, -1, 0
        if a ~= 0 then
            hp, bar, acc = u16(a + 0x14C), u16(a + 0x172), i32(a + 0x10)
            if hp > 0 and hp ~= bar and acc == 0 then
                absorb_since[s] = absorb_since[s] + 1
                any_absorb = true
                if absorb_since[s] == ABSORB_N and not triggered then
                    triggered = true
                    note = string.format("DESYNC slot %d", s)
                    write_wedge("SURVIVING DESYNC", string.format(
                        "slot %d absorbing %d vsyncs: hp=%d bar=%d acc=0",
                        s, ABSORB_N, hp, bar))
                end
            else
                absorb_since[s] = 0
            end
        end
        row[#row + 1] = hp; row[#row + 1] = bar; row[#row + 1] = acc
    end

    if park_since == PARK_N and not triggered then
        triggered = true
        note = string.format("PARK 0x%02X", ctx7)
        write_wedge(string.format("PARK (ctx7=0x%02X held %d vsyncs)", ctx7, PARK_N),
            string.format("c6d8=%d c276=%d", c6d8, c276))
    end
    if triggered and park_since == 0 and not any_absorb then
        triggered = false
    end

    row[#row + 1] = note
    local sig = table.concat(row, ",", 2, #row)
    if sig ~= last_sig or vsync % HEARTBEAT == 0 or note ~= "" then
        last_sig = sig
        csv:row("%d,%s,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s",
            row[1], row[2], row[3], row[4], row[5], row[6], row[7], row[8],
            row[9], row[10], row[11], row[12], row[13], row[14], row[15])
    end
end

-- keep the handle: a GC'd listener object deletes the C++ listener
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
PCSX.log(string.format(
    "[hunter] park hunter armed (poll-only, dynarec-safe): park_n=%d absorb_n=%d pause=%s",
    PARK_N, ABSORB_N, tostring(PAUSE)))
PCSX.log("[hunter] play freely; on detection the emulator PAUSES - savestate the moment, then resume")
