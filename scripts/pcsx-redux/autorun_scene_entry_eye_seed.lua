-- autorun_scene_entry_eye_seed.lua
--
-- Two routines seed the field camera's eye-space translation trio
-- (`_DAT_800840B8/BC/C0`) on a scene entry, and they disagree:
--
--   FUN_80025C24 (SCUS)        eye = (0, -0x100, 0x4024), angles (0x1B8, 0x64, 0)
--   FUN_801DE37C (field 0897)  eye = (0,  0x200, 0x4000), angles (0x1B8, 0x64, 0)
--
-- Both write the same angle trio, so only the eye differs - and only the
-- one that runs LAST stands when the view builder `FUN_800172C0` reads the
-- words on the first field frame. The disassembly says the order is fixed:
-- the field MAIN INIT `FUN_801D6704` calls `FUN_80025C24` at `0x801D698C`
-- and then, 0x41C bytes later at `0x801D6DA8`, `FUN_8003AEB0`, whose own
-- `jal 0x801DE37C` sits at `0x8003B01C`; no branch in either body skips
-- either call site. This probe measures that live across real scene
-- entries, and reads what the first view build after each entry actually
-- put in `TR`.
--
-- Every tap is fingerprinted before anything rests on it: `0x801DE37C`
-- and `0x801D698C` are slot-A overlay VAs that the menu / battle / minigame
-- overlays alias, so the probe checks the word at each one against the
-- field overlay's own byte before counting a hit.
--
-- Pad plan: each of the catalogued pre-transition states is one held
-- direction away from a scene change. `LEGAIA_HOLD_BTN` names it, and the
-- hold repeats every `LEGAIA_HOLD_PERIOD` vsyncs so a run that misses the
-- band on the first try gets more attempts inside one capture.
--
-- Env vars:
--   LEGAIA_SSTATE       save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES       capture vsyncs (default 1800)
--   LEGAIA_HOLD_BTN     UP / DOWN / LEFT / RIGHT / NONE (default UP)
--   LEGAIA_HOLD_START   first vsync of the first hold (default 60)
--   LEGAIA_HOLD_LEN     vsyncs held per attempt (default 90)
--   LEGAIA_HOLD_PERIOD  vsyncs between attempts (default 300)
--   LEGAIA_VIEW_BUDGET  view-builder hits recorded after each entry (default 3)
--   LEGAIA_OUT_DIR      output directory
--
-- Outputs: scene_entry_eye_seed.csv, scene_entry_eye_seed.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local HOLD_BTN    = probe.getenv("LEGAIA_HOLD_BTN", "UP")
local HOLD_START  = probe.getenv_num("LEGAIA_HOLD_START", 60)
local HOLD_LEN    = probe.getenv_num("LEGAIA_HOLD_LEN", 90)
local HOLD_PERIOD = probe.getenv_num("LEGAIA_HOLD_PERIOD", 300)
local VIEW_BUDGET = probe.getenv_num("LEGAIA_VIEW_BUDGET", 3)

local OUT_CSV = probe.out_path("scene_entry_eye_seed.csv")
local OUT_LOG = probe.out_path("scene_entry_eye_seed.log")

-- ---------------------------------------------------------------- addresses
local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local EYE_X, EYE_Y, EYE_Z = 0x800840B8, 0x800840BC, 0x800840C0
local PITCH, YAW, ROLL    = 0x8007B790, 0x8007B792, 0x8007B794
local VIEW_WINDOW         = 0x1F8003E8   -- scratchpad, seeded by FUN_801DE37C

-- Taps. `want` is the first word of the instruction at that VA, read from the
-- extracted image: a slot-A overlay alias fails this and is reported rather
-- than counted.
local TAPS = {
    -- FUN_80025C24 (SCUS): entry, and its `jr ra` - the trio is complete there.
    { addr = 0x80025C24, want = 0x3C028008, name = "FUN_80025C24 entry",   kind = "seed_a_in" },
    { addr = 0x80025C60, want = 0x03E00008, name = "FUN_80025C24 jr ra",   kind = "seed_a" },
    -- FUN_801DE37C (field overlay 0897): entry and `jr ra`.
    { addr = 0x801DE37C, want = 0x3C041F80, name = "FUN_801DE37C entry",   kind = "seed_b_in" },
    -- NB the `jr ra` fires BEFORE its delay slot, and `FUN_801DE37C`'s delay
    -- slot is the fourth visible-tile-window store (`sb v0, 0xd7(a0)`), so
    -- `vwin3` on a `seed_b` row is still the previous value.
    { addr = 0x801DE3D8, want = 0x03E00008, name = "FUN_801DE37C jr ra",   kind = "seed_b" },
    -- The three call sites, so the ORDER is measured at the callers too.
    { addr = 0x801D698C, want = 0x0C009709, name = "jal 80025C24 @ MAIN INIT", kind = "call_a" },
    { addr = 0x801D6DA8, want = 0x0C00EBAC, name = "jal 8003AEB0 @ MAIN INIT", kind = "call_ab" },
    { addr = 0x8003B01C, want = 0x0C0778DF, name = "jal 801DE37C @ FUN_8003AEB0", kind = "call_b" },
    -- The field view builder and the tap right after it uploads the eye trio
    -- verbatim as GTE `TR` (`FUN_8003D1A4` at 0x80017340).
    { addr = 0x800172C0, want = 0x27BDFFD8, name = "FUN_800172C0 entry",   kind = "view" },
    { addr = 0x80017348, want = 0x27A40018, name = "after TR = eye trio",  kind = "view_tr" },
}

-- ------------------------------------------------------------------ helpers
local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[eye_seed] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function s8(v)
    v = bit.band(tonumber(v) or 0, 0xFF)
    if v >= 0x80 then v = v - 0x100 end
    return v
end

local function s32(v)
    v = n32(v)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

-- Byte `i` of the scratchpad visible-tile window, sign-extended.
local function vwin(i)
    local w = n32(probe.read_scratch_u32(VIEW_WINDOW) or 0)
    return s8(bit.band(bit.rshift(w, i * 8), 0xFF))
end

local function cp2c_tr()
    local r = PCSX.getRegisters()
    return s32(r.CP2C.r[5]), s32(r.CP2C.r[6]), s32(r.CP2C.r[7])
end

-- ----------------------------------------------------------------- csv shape
local CSV_HEADER = table.concat({
    "seq", "vsync", "event", "pc", "ra",
    "eye_x", "eye_y", "eye_z",
    "pitch", "yaw", "roll",
    "tr_x", "tr_y", "tr_z",
    "vwin0", "vwin1", "vwin2", "vwin3",
    "scene", "mode",
}, ",")

local csv = nil
local seq = 0
local g_elapsed = 0
local hits = {}
-- Seeded, not zero: a run with no scene entry in it still records its first
-- `LEGAIA_VIEW_BUDGET` view builds, which is how the per-frame call count of
-- `FUN_800172C0` gets measured.
local view_budget = VIEW_BUDGET
local entry_count = 0
local last_event_vsync = -1000
local order = {}          -- the kind sequence within each entry window
local scene_log = {}
local last_scene = nil
local fingerprint_ok = {}
local hold_held = nil

local function row(kind, pc, extra)
    seq = seq + 1
    local r = PCSX.getRegisters()
    local trx, try, trz = 0, 0, 0
    if kind == "view_tr" then trx, try, trz = cp2c_tr() end
    local vals = {
        seq, g_elapsed, kind, hex8(pc), hex8(r.GPR.n.ra),
        s32(probe.read_u32(EYE_X) or 0),
        s32(probe.read_u32(EYE_Y) or 0),
        s32(probe.read_u32(EYE_Z) or 0),
        s16(probe.read_u16(PITCH) or 0),
        s16(probe.read_u16(YAW) or 0),
        s16(probe.read_u16(ROLL) or 0),
        trx, try, trz,
        -- The visible-tile window is SCRATCHPAD (`0x1F8003E8..EB`, word
        -- aligned): the main-RAM reader returns nothing there, so it is one
        -- scratchpad word split into its four signed bytes, not four
        -- `read_u8` calls - those read as zeros.
        vwin(0), vwin(1), vwin(2), vwin(3),
        scene_name(), string.format("0x%02X", probe.read_u8(GAME_MODE) or 0),
    }
    for i, v in ipairs(vals) do vals[i] = tostring(v) end
    if csv then csv:row("%s", table.concat(vals, ",")) end
    if extra then logf("%s", extra) end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV, CSV_HEADER)
        probe.write_manifest("autorun_scene_entry_eye_seed.lua", {
            sstate = SSTATE, frames = FRAMES, hold_btn = HOLD_BTN,
            hold_start = HOLD_START, hold_len = HOLD_LEN,
            hold_period = HOLD_PERIOD, view_budget = VIEW_BUDGET,
        })
        local descs = {}
        for _, tap in ipairs(TAPS) do
            local t = tap
            hits[t.kind] = 0
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            probe.arm_breakpoint(t.addr, "Exec", 4,
                string.format("eye_%08X", t.addr), function()
                -- A slot-A alias executes different bytes at the same VA;
                -- only count a hit whose instruction word is the one the
                -- field image carries there.
                if t.want ~= nil and n32(probe.read_u32(t.addr) or 0) ~= t.want then
                    hits[t.kind .. "_alias"] = (hits[t.kind .. "_alias"] or 0) + 1
                    return
                end
                if t.kind == "view" or t.kind == "view_tr" then
                    if view_budget <= 0 then return end
                    if t.kind == "view_tr" then view_budget = view_budget - 1 end
                else
                    -- A seed / call-site hit opens a new entry window when the
                    -- previous one went quiet, so the order is recorded even if
                    -- the routine this lane expects to run first does not.
                    if entry_count == 0 or (g_elapsed - last_event_vsync) > 120 then
                        entry_count = entry_count + 1
                        order[entry_count] = {}
                        view_budget = VIEW_BUDGET
                    end
                    last_event_vsync = g_elapsed
                    local w = order[entry_count]
                    if w then w[#w + 1] = t.kind end
                end
                hits[t.kind] = hits[t.kind] + 1
                d.hits_ref.n = hits[t.kind]
                row(t.kind, t.addr)
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            for _, t in ipairs(TAPS) do
                if t.want ~= nil then
                    local w = n32(probe.read_u32(t.addr) or 0)
                    fingerprint_ok[t.addr] = (w == t.want)
                    logf("fingerprint [0x%08X] = 0x%s (want 0x%s) %s -- %s",
                         t.addr, hex8(w), hex8(t.want),
                         (w == t.want) and "OK" or "ALIAS", t.name)
                end
            end
            logf("start: scene=%s mode=0x%02X eye=(%d,%d,%d)",
                 scene_name(), probe.read_u8(GAME_MODE) or 0,
                 s32(probe.read_u32(EYE_X) or 0),
                 s32(probe.read_u32(EYE_Y) or 0),
                 s32(probe.read_u32(EYE_Z) or 0))
        end

        local sc = scene_name()
        if sc ~= last_scene then
            scene_log[#scene_log + 1] = string.format("v%d:%s", elapsed, sc)
            logf("scene word -> %s at vsync %d (mode 0x%02X)", sc, elapsed,
                 probe.read_u8(GAME_MODE) or 0)
            last_scene = sc
        end

        if HOLD_BTN ~= "NONE" then
            local btn = probe.BTN[HOLD_BTN]
            if btn ~= nil and elapsed >= HOLD_START then
                local phase = (elapsed - HOLD_START) % HOLD_PERIOD
                local want = phase < HOLD_LEN
                if want and hold_held == nil then
                    probe.pad_force(btn); hold_held = btn
                elseif not want and hold_held ~= nil then
                    probe.pad_release(hold_held); hold_held = nil
                end
            end
        end
    end,

    on_summary = function(ctx, descs)
        if hold_held then probe.pad_release(hold_held); hold_held = nil end
        local parts = {}
        for _, t in ipairs(TAPS) do
            parts[#parts + 1] = string.format("%s=%d%s", t.kind, hits[t.kind] or 0,
                (hits[t.kind .. "_alias"] or 0) > 0
                    and string.format("(+%d alias)", hits[t.kind .. "_alias"]) or "")
        end
        logf("hits: %s", table.concat(parts, " "))
        logf("scene word timeline: %s", table.concat(scene_log, " "))
        logf("scene entries seen: %d", entry_count)
        for i = 1, entry_count do
            logf("  entry %d order: %s", i, table.concat(order[i] or {}, " -> "))
        end
        local fh = io.open(OUT_LOG, "w")
        if fh then
            fh:write(table.concat(lines, "\n")); fh:write("\n"); fh:close()
        end
        if csv then csv:close() end
    end,
})
