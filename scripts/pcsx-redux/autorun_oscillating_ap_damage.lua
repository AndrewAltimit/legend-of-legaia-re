-- autorun_oscillating_ap_damage.lua
--
-- Runtime check for the **oscillating AP costs** patch's grant-side damage
-- scale: does the detour at the arms execution resolver's damage site
-- (0x801EDA10 -> SCUS_GAP) recognise the executing art from the kernel's
-- action entry and scale `s0 - s1` by DAMAGE_PCT?
--
-- Recipe: run the emulator on the PATCHED disc, resume a retail-captured
-- battle state that executes arts on its own (default: the Tri-Somersault
-- counterattack chain, `battle_vahn_tri_somersault_super`), apply the
-- `legaia-patcher scus-pokes` list so the resident SCUS carries the routines,
-- poke the four 0898 detours into the resident battle overlay (a state's
-- overlay is the retail one), and force every side bit to GRANT so any art
-- that lands must scale. Exec breakpoints on the routine's entry and its
-- return word log the entry pointer, the row the routine derives (from the
-- art bank at record0[+0x58]) and `s0 - s1` before / after.
--
-- PASS: every strike whose entry is an art-bank record scales to exactly
-- floor(dmg * pct / 100), and every other strike (a direction swing, a chain
-- connector) is untouched.
--
-- Env: LEGAIA_POKES (required), LEGAIA_SSTATE, LEGAIA_OUT_DIR,
--      LEGAIA_PCT (default 20), LEGAIA_FORCE_GRANT (default 1).
-- Output: <OUT_DIR>/oscillating_ap_damage.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local step  = require("probe.step")

local DMG_VA   = 0x80077728   -- SCUS_GAP: the damage routine
local RET_VA   = 0x801EDA18   -- the kernel word the routine returns to
local BITS_VA  = 0x800777C4   -- side table (damage routine + 39 words)
local NUM_ROWS = 26

-- The plan's 0898 detours (`j routine; nop`), poked because the resident
-- overlay came from the retail disc that captured the state.
local OV_DETOURS = {
    { 0x801EF410, 0x0801EB90 }, -- guard  -> 0x8007AE40
    { 0x801EF490, 0x0801EB9B }, -- debit  -> 0x8007AE6C
    { 0x801EF988, 0x0801EBFE }, -- refund -> 0x8007AFF8
    { 0x801EDA10, 0x0801DDCA }, -- damage -> 0x80077728
}

local POKES_PATH  = probe.getenv("LEGAIA_POKES", "")
local PCT         = probe.getenv_num("LEGAIA_PCT", 20)
local FORCE_GRANT = probe.getenv_num("LEGAIA_FORCE_GRANT", 1)

local report = {}
local function say(fmt, ...)
    local line = select("#", ...) > 0 and string.format(fmt, ...) or fmt
    report[#report + 1] = line
    PCSX.log("[oscdmg] " .. line)
end

local function read_pokes(path)
    local out = {}
    local f = io.open(path, "r")
    if not f then return nil end
    for line in f:lines() do
        local a, w = line:match("^%s*0x(%x+)%s*:%s*0x(%x+)")
        if a then out[#out + 1] = { addr = tonumber(a, 16), word = tonumber(w, 16) } end
    end
    f:close()
    return out
end

local strikes = {}      -- in order: { slot, entry, row, before, after }
local hits = { n = 0 }
local pending = nil
local last_v = 0
local first_hit_v = nil

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1200),
    boot_delay     = 90,
    quit_delay     = 10,

    on_arm = function(ctx)
        bp.arm(DMG_VA, "Exec", 4, "osc_dmg", function()
            hits.n = hits.n + 1
            first_hit_v = first_hit_v or last_v
            local r = step.regs()
            local slot = bit.band(r.s6, 0xff)
            local entry = mem.read_u32(r.sp + 0x54)
            local row = nil
            if slot < 3 then
                local rec0 = mem.read_u32(0x801C9360 + slot * 4)
                local bank = mem.read_u32(rec0 + 0x58)
                local delta = entry - bank - 0x28
                if delta >= 0 and delta % 0xD0 == 0 then row = math.floor(delta / 0xD0) - 0x0B end
            end
            pending = { slot = slot, entry = entry, row = row, before = r.s0 - r.s1 }
            say("v=%d strike: slot=%d entry=%08X row=%s dmg_in=%d", last_v, slot, entry,
                row and tostring(row) or "-", pending.before)
        end)
        bp.arm(RET_VA, "Exec", 4, "osc_ret", function()
            if not pending then return end
            local r = step.regs()
            pending.after = r.s0 - r.s1
            strikes[#strikes + 1] = pending
            say("   -> dmg_out=%d", pending.after)
            pending = nil
        end)
        return {
            { addr = DMG_VA, name = "damage routine", hits_ref = hits },
        }
    end,

    on_capture = function(ctx, v)
        last_v = v
        if v == 2 then
            local pokes = POKES_PATH ~= "" and read_pokes(POKES_PATH) or nil
            if not pokes or #pokes == 0 then
                say("FAIL: no pokes read from %q - resident SCUS is unpatched", POKES_PATH)
                ctx.request_quit = true
                return
            end
            for _, p in ipairs(pokes) do mem.write_u32(p.addr, p.word) end
            for _, d in ipairs(OV_DETOURS) do
                mem.write_u32(d[1], d[2])
                mem.write_u32(d[1] + 4, 0)
            end
            if FORCE_GRANT == 1 then
                for i = 0, 15 do mem.write_u8(BITS_VA + i, 0xFF) end
            end
            say("applied %d SCUS pokes + 4 overlay detours; routine word %08X; site word %08X; table = %s",
                #pokes, mem.read_u32(DMG_VA), mem.read_u32(0x801EDA10),
                mem.bytes_to_hex(mem.read_bytes(BITS_VA, 16)))
        end
        if first_hit_v and v > first_hit_v + 600 then ctx.request_quit = true end
    end,

    on_done = function(ctx)
        local scaled, kept, bad = 0, 0, 0
        for _, s in ipairs(strikes) do
            local is_art = s.row ~= nil and s.row >= 0 and s.row < NUM_ROWS and s.slot < 3
            local want = (is_art and FORCE_GRANT == 1) and math.floor(s.before * PCT / 100) or s.before
            if s.after == want then
                if want ~= s.before then scaled = scaled + 1 else kept = kept + 1 end
            else
                bad = bad + 1
                say("MISMATCH: row=%s before=%d after=%d wanted=%d", tostring(s.row), s.before, s.after, want)
            end
        end
        say("strikes: %d (scaled %d, kept %d, wrong %d)", #strikes, scaled, kept, bad)
        if #strikes == 0 then
            say("INCONCLUSIVE: no strike reached the damage site in the capture window")
        elseif bad == 0 and (FORCE_GRANT ~= 1 or scaled > 0) then
            -- No literal percent sign in a logged line: PCSX.log treats it as a directive.
            say("PASS: grant-side arts scale to %d per cent, everything else is untouched", PCT)
        else
            say("FAIL: see the mismatches above")
        end
        local path = probe.out_path("oscillating_ap_damage.txt")
        local f = io.open(path, "w")
        if f then f:write(table.concat(report, "\n"), "\n"); f:close(); PCSX.log("[oscdmg] wrote " .. path) end
    end,
})
