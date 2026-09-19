-- autorun_field_submode_park.lua
--
-- What does a field submode return to - and does anything ever read the word
-- the return state is parked in?
--
-- The op-`0x49` submode enter is two copies of one idiom in the field overlay
-- (PROT 0897, base 0x801CE818). Both load the scene struct through the pointer
-- at `0x801C6EA4`, stamp `scene[+0x2E] = -1`, copy the driver's current handler
-- slot `s4[+0x50]` into `scene[+0x40]`, and then overwrite `s4[+0x50]`:
--
--   0x801F1400  sh v0,0x40(v1)   park  ]  first copy: parks the PRE-ENTER slot,
--   0x801F140C  sh v0,0x50(s4)   = 7   ]  installs the submode RETURN state 7
--   0x801F148C  sh v0,0x40(v1)   park  ]  second copy (taken when the op-0x49
--   0x801F14AC  sh v0,0x50(s4)   = tbl ]  install pointer 0x8007B450 is live):
--                                        parks the 7 just installed and
--                                        overwrites the slot with the sub-op's
--                                        own, off the table at 0x801F33A4.
--
-- A static sweep of SCUS_942.54 and every extracted overlay finds 26 writes of
-- `scene[+0x40]` through that pointer and NO read of it. A register-copy of the
-- struct pointer would hide a reader from that sweep, so this probe settles it
-- at runtime: a hardware READ watch on the live `scene + 0x40` (and on
-- `scene + 0x2E`) across a submode enter, plus an exec tap on all four sites
-- so the parked value is recorded as it is written.
--
-- Env vars:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 1800)
--   LEGAIA_HOLD_BTN    button tapped to drive the transition (default CROSS)
--   LEGAIA_HOLD_PERIOD vsyncs between taps (default 20)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: field_submode_park.csv, field_submode_park.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local HOLD_BTN    = probe.getenv("LEGAIA_HOLD_BTN", "CROSS")
local HOLD_PERIOD = probe.getenv_num("LEGAIA_HOLD_PERIOD", 20)

local OUT_CSV = probe.out_path("field_submode_park.csv")
local OUT_LOG = probe.out_path("field_submode_park.log")

local SCENE_PTR   = 0x801C6EA4   -- -> the field scene struct
local SCENE_NAME  = 0x8007050C
local GAME_MODE   = 0x8007B83C
local OP49_INSTALL= 0x8007B450   -- _DAT_8007B450, the op-0x49 install pointer
local SUBMODE_ST  = 0x801F2734   -- submode context word 0 (1 = open)

local TAPS = {
    { addr = 0x801F13EC, want = 0x8C436EA4, kind = "enter_head", name = "enter: load scene ptr" },
    { addr = 0x801F1400, want = 0xA4620040, kind = "park_a",     name = "park pre-enter slot" },
    { addr = 0x801F140C, want = 0xA6820050, kind = "install_7",  name = "+0x50 = 7 (return state)" },
    { addr = 0x801F148C, want = 0xA4620040, kind = "park_b",     name = "park the 7" },
    { addr = 0x801F14AC, want = 0xA6820050, kind = "install_sub",name = "+0x50 = sub-op slot" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[submode] " .. s)
end

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function scene_ptr()
    local p = u32(SCENE_PTR)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
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

local csv
local g_elapsed = 0
local seq = 0
local armed_ptr = nil
local n_reads_40, n_reads_2e, n_writes_40 = 0, 0, 0
local reader_pcs = {}
local tap_hits = {}
local last = { st = -1, p2e = -9999, p40 = -9999, s50 = -9999, mode = -1, scene = "" }

local function row(kind, pc, ra, a, b, note)
    seq = seq + 1
    local sp = scene_ptr()
    csv:row("%d,%d,%s,%s,%s,%s,%s,%s,%d,%d,%s,%s,0x%02X,%s",
        seq, g_elapsed, kind, pc or "", ra or "", a or "", b or "",
        sp and ("0x" .. hex8(sp)) or "",
        sp and s16(u16(sp + 0x2E)) or -9999,
        sp and s16(u16(sp + 0x40)) or -9999,
        "0x" .. hex8(u32(OP49_INSTALL)),
        "0x" .. hex8(u32(SUBMODE_ST)),
        u8(GAME_MODE), scene_name())
    if note then logf("%s", note) end
end

local function arm_watches(p)
    -- Two-byte read watch on the parked word and on the scene state byte pair.
    probe.arm_breakpoint(p + 0x40, "Read", 2, "read_park", function()
        n_reads_40 = n_reads_40 + 1
        local r = PCSX.getRegisters()
        local pc = n32(r.pc)
        reader_pcs[pc] = (reader_pcs[pc] or 0) + 1
        if n_reads_40 <= 200 then
            row("READ_scene_0x40", "0x" .. hex8(pc), "0x" .. hex8(r.GPR.n.ra))
        end
    end)
    probe.arm_breakpoint(p + 0x2E, "Read", 2, "read_2e", function()
        n_reads_2e = n_reads_2e + 1
        if n_reads_2e <= 60 then
            local r = PCSX.getRegisters()
            row("READ_scene_0x2E", "0x" .. hex8(n32(r.pc)), "0x" .. hex8(r.GPR.n.ra))
        end
    end)
    probe.arm_breakpoint(p + 0x40, "Write", 2, "write_park", function()
        n_writes_40 = n_writes_40 + 1
        local r = PCSX.getRegisters()
        if n_writes_40 <= 200 then
            row("WRITE_scene_0x40", "0x" .. hex8(n32(r.pc)), "0x" .. hex8(r.GPR.n.ra))
        end
    end)
    armed_ptr = p
    logf("armed read/write watches on scene 0x%s (+0x2E, +0x40) at vsync %d",
         hex8(p), g_elapsed)
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "seq,vsync,event,pc,ra,a,b,scene_ptr,scene_2e,scene_40," ..
            "op49_install,submode_state,mode,scene")
        probe.env.write_manifest("autorun_field_submode_park.lua", {
            sstate = SSTATE, frames = FRAMES, hold_btn = HOLD_BTN,
            hold_period = HOLD_PERIOD,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            tap_hits[t.kind] = 0
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                tap_hits[t.kind] = tap_hits[t.kind] + 1
                local r = PCSX.getRegisters()
                -- v0 carries the value being stored at both park and install
                -- sites; s4 is the driver struct.
                row(t.kind, "0x" .. hex8(t.addr), "0x" .. hex8(r.GPR.n.ra),
                    "v0=" .. tostring(n32(r.GPR.n.v0)),
                    "s4=0x" .. hex8(r.GPR.n.s4))
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed

        if elapsed == 2 then
            for _, t in ipairs(TAPS) do
                local got = u32(t.addr)
                logf("fingerprint [0x%08X] = 0x%s (want 0x%s, %s) %s",
                     t.addr, hex8(got), hex8(t.want), t.name,
                     got == t.want and "OK" or "MISMATCH")
            end
            logf("scene ptr [0x801C6EA4] = 0x%s scene=%s mode=0x%02X",
                 hex8(u32(SCENE_PTR)), scene_name(), u8(GAME_MODE))
        end

        local p = scene_ptr()
        if p and p ~= armed_ptr then arm_watches(p) end

        probe.pad_release(probe.BTN[HOLD_BTN])
        local sub = elapsed % HOLD_PERIOD
        if sub >= math.floor(HOLD_PERIOD / 2) and
           sub < math.floor(HOLD_PERIOD / 2) + 3 then
            probe.pad_force(probe.BTN[HOLD_BTN])
        end

        -- Log every transition of the words this row is about.
        local st = u32(SUBMODE_ST)
        local p2e = p and s16(u16(p + 0x2E)) or -9999
        local p40 = p and s16(u16(p + 0x40)) or -9999
        local mode = u8(GAME_MODE)
        local sc = scene_name()
        if st ~= last.st or p2e ~= last.p2e or p40 ~= last.p40 or
           mode ~= last.mode or sc ~= last.scene then
            row("poll", "", "", "",
                string.format("prev 2e=%d 40=%d st=0x%s mode=0x%02X",
                    last.p2e, last.p40, hex8(last.st), last.mode))
            last.st, last.p2e, last.p40 = st, p2e, p40
            last.mode, last.scene = mode, sc
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN[HOLD_BTN])
        local th = {}
        for k, v in pairs(tap_hits) do th[#th + 1] = string.format("%s=%d", k, v) end
        table.sort(th)
        logf("exec taps: %s", table.concat(th, " "))
        logf("scene[+0x40]: reads=%d writes=%d ; scene[+0x2E] reads=%d",
             n_reads_40, n_writes_40, n_reads_2e)
        local rp = {}
        for pc, n in pairs(reader_pcs) do
            rp[#rp + 1] = string.format("0x%s x%d", hex8(pc), n)
        end
        table.sort(rp)
        logf("distinct PCs that READ scene[+0x40]: %s",
             #rp > 0 and table.concat(rp, " ") or "(none)")
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
