-- autorun_w4d_melee_grunt_gate.lua
--
-- Which `s7` definition does an ordinary party swing carry into the melee
-- grunt gate at `0x801EEAA0`?
--
-- `FUN_801EC3E4` (PROT 0898, base `0x801CE818`) guards the `XA30` grunt with
-- three tests in a row:
--
--   801EEA7C  sltiu v0,a0,0x3        ; a0 = s6 = seat; party seats only
--   801EEA88  beq   v1,zero,...      ; v1 = s7 & 0xFF; s7 != 0
--   801EEA98  lbu   v0,0x1f3(v0)     ; v0 = actor[s4]
--   801EEAA0  bne   v1,v0,...        ; s7 == actor[s4][+0x1F3]
--   801EEAB4  slti  v0,v0,0x2        ; _DAT_8007BC20 < 2
--   801EEAC8  bne   v0,zero,...      ; _DAT_8007BD84 == 0 -> grunt, else sting
--
-- Fourteen definitions of `s7` reach that compare and only one of them loads
-- `+0x1F3` (`0x801EC884`); the rest load `+0x1EF`/`+0x1F0`/`+0x1F1`, or set a
-- constant. Which one is live on a plain Attack is a runtime property, so this
-- probe arms an Exec BP on every definition site and records the LAST one that
-- executed before each visit to the gate - the live reaching definition,
-- measured rather than argued.
--
-- Also armed: the two emission sites (`0x801EEB44` grunt `FUN_8003D53C(0x1D,
-- chan, dur)` and `0x801EEBE8` sting `FUN_8004FE5C(0x10C, seat)`) and the
-- `+0x1DA` commit at `0x801EEC6C` that stages the pose byte the gate compares.
--
-- Note the addresses are slot-A overlay VAs: they are only the battle-action
-- overlay while PROT 0898 is resident, which is why this probe must be driven
-- from a battle save state and not from the field.
--
-- Env vars:
--   LEGAIA_SSTATE   save state (--scenario party_basic_attack_vs_gobu_gobu or
--                   rim_elm_queen_bee_battle)
--   LEGAIA_FRAMES   capture vsyncs (default 1500)
--   LEGAIA_ADVANCE  0 = no pad, 1 = Cross on a cadence (default),
--                   2 = Cross + Left interleaved (drives an arts chain too)
--   LEGAIA_OUT_DIR  output directory
--
-- Outputs: w4d_grunt_gate.csv, w4d_grunt_gate.log, w4d_grunt_gate.detail.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE  = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 1500)
local ADVANCE = probe.getenv_num("LEGAIA_ADVANCE", 1)

local OUT_LOG    = probe.out_path("w4d_grunt_gate.log")
local OUT_CSV    = probe.out_path("w4d_grunt_gate.csv")
local OUT_DETAIL = probe.out_path("w4d_grunt_gate.detail.txt")

local ACTOR_TABLE = 0x801C9370   -- DAT_801C9370[seat] -> battle actor
local CHAR_IDS    = 0x8007BD10   -- DAT_8007BD10[seat] -> character id
local LEVEL_GATE  = 0x8007BC20   -- _DAT_8007BC20, the < 2 mute
local FX_HANDLE   = 0x8007BD84   -- _DAT_8007BD84, effect-instance handle
local GAME_MODE   = 0x8007B83C

local GATE_S7NZ   = 0x801EEA88   -- beq v1,zero  (s7 != 0)
local GATE_MATCH  = 0x801EEAA0   -- bne v1,v0    (s7 == actor[s4][+0x1F3])
local GRUNT_CALL  = 0x801EEB44   -- jal FUN_8003D53C
local STING_CALL  = 0x801EEBE8   -- jal FUN_8004FE5C
local POSE_COMMIT = 0x801EEC6C   -- sb s7,0x1da(v1)

-- Every instruction in FUN_801EC3E4 that defines s7, read off the
-- disassembly (ghidra/scripts/funcs/overlay_0898_801ec3e4.txt).
local S7_DEFS = {
    { addr = 0x801EC590, what = "clear s7" },
    { addr = 0x801EC884, what = "lbu s7,0x1f3(v1)" },
    { addr = 0x801EC984, what = "move s7,a1" },
    { addr = 0x801ECA68, what = "clear s7" },
    { addr = 0x801ECAC4, what = "li s7,0x1" },
    { addr = 0x801ECB5C, what = "clear s7" },
    { addr = 0x801ED324, what = "lui s7,0x8008" },
    { addr = 0x801EDE5C, what = "lbu s7,0x1ef(v0)" },
    { addr = 0x801EDE78, what = "lbu s7,0x1f0(v0)" },
    { addr = 0x801EDEA8, what = "lbu s7,0x1ef(v0)" },
    { addr = 0x801EDEBC, what = "move s7,v1" },
    { addr = 0x801EE304, what = "lbu s7,0x1f1(v1)" },
    { addr = 0x801EE32C, what = "li s7,0x2" },
    { addr = 0x801EE374, what = "lbu s7,0x1f1(v0)" },
    { addr = 0x801EE3B4, what = "lbu s7,0x1f1(v1)" },
    { addr = 0x801EEBD4, what = "clear s7" },
    { addr = 0x801EEC30, what = "lbu s7,0x1f3(a0)" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[w4d_grunt] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end

local function actor_ptr(seat)
    if seat == nil or seat < 0 or seat > 7 then return nil end
    local p = probe.read_u32(ACTOR_TABLE + seat * 4)
    if p == nil or p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local csv = nil
local g_elapsed = 0
local last_def, last_def_tick = "none", -1
local n_gate, n_match, n_grunt, n_sting, n_commit = 0, 0, 0, 0, 0
local cross_held, left_held = false, false

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV,
            "tick,event,s7,s4,s6,p1f3,p1da,p1db,p1de,lvl,fx,lastdef,note")
        ctx.def_hits  = {}
        ctx.gate_rows = {}
        probe.write_manifest("autorun_w4d_melee_grunt_gate.lua", {
            sstate = SSTATE, frames = FRAMES, advance = ADVANCE,
            core = probe.getenv("LEGAIA_CORE", "?"),
        })

        for _, d in ipairs(S7_DEFS) do
            local label = string.format("0x%08X %s", d.addr, d.what)
            probe.arm_breakpoint(d.addr, "Exec", 4, "s7def_" ..
                string.format("%08X", d.addr), function()
                ctx.def_hits[label] = (ctx.def_hits[label] or 0) + 1
                last_def, last_def_tick = label, g_elapsed
            end)
        end

        -- The gate itself. `v1` is s7 & 0xFF (set in the delay slot at
        -- 0x801EEA84), `s4` the sting seat, `s6` the grunt seat.
        probe.arm_breakpoint(GATE_S7NZ, "Exec", 4, "gate_s7nz", function()
            n_gate = n_gate + 1
            local r  = PCSX.getRegisters()
            local v1 = bit.band(n32(r.GPR.n.v1), 0xFF)
            local s4 = bit.band(n32(r.GPR.n.s4), 0xFF)
            local s6 = bit.band(n32(r.GPR.n.s6), 0xFF)
            local s7 = bit.band(n32(r.GPR.n.s7), 0xFF)
            local a  = actor_ptr(s4)
            local p1f3 = a and (probe.read_u8(a + 0x1F3) or 0) or -1
            local p1da = a and (probe.read_u8(a + 0x1DA) or 0) or -1
            local p1db = a and (probe.read_u8(a + 0x1DB) or 0) or -1
            local p1de = a and (probe.read_u8(a + 0x1DE) or 0) or -1
            local lvl  = probe.read_u32(LEVEL_GATE) or 0
            local fx   = probe.read_u32(FX_HANDLE) or 0
            local verdict
            if v1 == 0 then verdict = "skip_s7_zero"
            elseif v1 ~= p1f3 then verdict = "skip_mismatch"
            else verdict = "pass_to_level_gate" end
            csv:row("%d,gate,0x%02X,%d,%d,0x%02X,0x%02X,0x%02X,0x%02X,%d,0x%08X,%s,%s",
                g_elapsed, s7, s4, s6, p1f3, p1da, p1db, p1de, lvl, fx,
                last_def, verdict)
            local key = string.format("s7=0x%02X +1F3=0x%02X seat(s6)=%d %s def=%s",
                s7, p1f3, s6, verdict, last_def)
            ctx.gate_rows[key] = (ctx.gate_rows[key] or 0) + 1
            if n_gate <= 12 then
                logf("GATE #%d vsync %d: s7=0x%02X v1=0x%02X s4=%d s6=%d +1F3=0x%02X +1DA=0x%02X +1DB=0x%02X +1DE=0x%02X lvl=%d fx=0x%08X -> %s (last s7 def %s @%d)",
                    n_gate, g_elapsed, s7, v1, s4, s6, p1f3, p1da, p1db, p1de,
                    lvl, fx, verdict, last_def, last_def_tick)
                probe.append_call_context(OUT_DETAIL,
                    probe.capture_call_context(string.format(
                        "grunt gate #%d vsync=%d verdict=%s lastdef=%s",
                        n_gate, g_elapsed, verdict, last_def)))
            end
        end)

        probe.arm_breakpoint(GATE_MATCH, "Exec", 4, "gate_match", function()
            n_match = n_match + 1
            local r = PCSX.getRegisters()
            csv:row("%d,match,0x%02X,,,0x%02X,,,,,,%s,reached_1F3_compare",
                g_elapsed, bit.band(n32(r.GPR.n.v1), 0xFF),
                bit.band(n32(r.GPR.n.v0), 0xFF), last_def)
        end)

        probe.arm_breakpoint(GRUNT_CALL, "Exec", 4, "grunt_call", function()
            n_grunt = n_grunt + 1
            local r = PCSX.getRegisters()
            local a0 = n32(r.GPR.n.a0)
            local a1 = n32(r.GPR.n.a1)
            local a2 = n32(r.GPR.n.a2)
            logf("GRUNT #%d vsync %d: FUN_8003D53C(clip=0x%02X, chan=%d, dur=0x%02X) seat_char=0x%02X",
                n_grunt, g_elapsed, a0, a1, a2,
                probe.read_u8(CHAR_IDS + bit.band(n32(r.GPR.n.s6), 0xFF)) or 0)
            csv:row("%d,grunt,,,,,,,,,,%s,clip=0x%02X chan=%d dur=0x%02X",
                g_elapsed, last_def, a0, a1, a2)
        end)

        probe.arm_breakpoint(STING_CALL, "Exec", 4, "sting_call", function()
            n_sting = n_sting + 1
            local r = PCSX.getRegisters()
            logf("STING #%d vsync %d: FUN_8004FE5C(0x%02X, seat=%d)",
                n_sting, g_elapsed, n32(r.GPR.n.a0), n32(r.GPR.n.a1))
            csv:row("%d,sting,,,,,,,,,,%s,cue=0x%02X seat=%d",
                g_elapsed, last_def, n32(r.GPR.n.a0), n32(r.GPR.n.a1))
        end)

        probe.arm_breakpoint(POSE_COMMIT, "Exec", 4, "pose_commit", function()
            n_commit = n_commit + 1
            local r = PCSX.getRegisters()
            if n_commit <= 12 then
                logf("COMMIT #%d vsync %d: actor+0x1DA <- 0x%02X (s7)",
                    n_commit, g_elapsed, bit.band(n32(r.GPR.n.s7), 0xFF))
            end
            csv:row("%d,commit,0x%02X,,,,,,,,,%s,sb s7 -> +0x1DA",
                g_elapsed, bit.band(n32(r.GPR.n.s7), 0xFF), last_def)
        end)

        local descs = {
            { addr = GATE_S7NZ,   name = "0x801EEA88 s7 != 0" },
            { addr = GATE_MATCH,  name = "0x801EEAA0 s7 == actor[s4]+0x1F3" },
            { addr = GRUNT_CALL,  name = "0x801EEB44 grunt FUN_8003D53C" },
            { addr = STING_CALL,  name = "0x801EEBE8 sting FUN_8004FE5C" },
            { addr = POSE_COMMIT, name = "0x801EEC6C sb s7 -> +0x1DA" },
        }
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        -- Cross on a cadence walks the command flow: Begin -> Attack ->
        -- Auto -> confirm, and then the swing runs. ADVANCE=2 interleaves
        -- Left, which is both the ring's Attack arm and, on the directional
        -- arts-entry screen, a left arm swing - so one cadence drives a plain
        -- Attack and an arts chain alike.
        if ADVANCE ~= 0 then
            local phase = elapsed % 30
            if phase == 0 and not cross_held then
                probe.pad_force(probe.BTN.CROSS); cross_held = true
            elseif phase == 8 and cross_held then
                probe.pad_release(probe.BTN.CROSS); cross_held = false
            elseif ADVANCE == 2 and phase == 15 and not left_held then
                probe.pad_force(probe.BTN.LEFT); left_held = true
            elseif ADVANCE == 2 and phase == 22 and left_held then
                probe.pad_release(probe.BTN.LEFT); left_held = false
            end
        end
        if (elapsed % 150) == 0 then
            logf("vsync %4d mode=0x%02X gate=%d match=%d grunt=%d sting=%d commit=%d lastdef=%s",
                elapsed, probe.read_u8(GAME_MODE) or 0, n_gate, n_match,
                n_grunt, n_sting, n_commit, last_def)
        end
    end,

    on_done = function(ctx)
        if cross_held then probe.pad_release(probe.BTN.CROSS) end
        if left_held then probe.pad_release(probe.BTN.LEFT) end
        logf("=== gate visits=%d  1F3-compare=%d  grunts=%d  stings=%d  commits=%d ===",
            n_gate, n_match, n_grunt, n_sting, n_commit)
        logf("--- s7 definition sites that executed ---")
        local keys = {}
        for k in pairs(ctx.def_hits) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do logf("  %s  x%d", k, ctx.def_hits[k]) end
        logf("--- distinct gate outcomes ---")
        keys = {}
        for k in pairs(ctx.gate_rows) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do logf("  %s  x%d", k, ctx.gate_rows[k]) end
        if csv then csv:close() end
        local f = io.open(OUT_LOG, "w")
        if f then
            f:write(table.concat(lines, "\n") .. "\n")
            f:close()
            PCSX.log("[w4d_grunt] wrote " .. OUT_LOG)
        end
    end,
})
