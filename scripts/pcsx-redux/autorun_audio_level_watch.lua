-- autorun_audio_level_watch.lua
--
-- Who writes the applied audio level DAT_8007B6EC, and how it follows the
-- duck cell _DAT_8007B910 under a summon (docs/subsystems/audio.md, "The
-- duck's volume is the level halved").
--
-- From a PRE-cast battle state, rewrite the acting seat's queued action into
-- a summon (the injection autorun_cast_sequence.lua uses: at action state
-- 0x0A write actor[+0x1DE] = 2, +0x1DF = spell, +0x1DD = target), then log
-- every write to either word with the writing pc / ra, and one row per vsync
-- with both words and the BGM record's sequence volume.
--
-- Needs the BP core (no --fast): the write watch is a Lua breakpoint.
--
-- Outputs (--out-dir):
--   writes.csv  vsync,word,pc,ra,old,new   (word = b910 | b6ec)
--   level.csv   vsync,st7,b910,b6ec,seqvol1,seqvol2,seqvol3
-- Retail RAM-derived: never commit them.
--
-- Env: LEGAIA_SPELL (0x86), LEGAIA_CASTER_SEAT (0), LEGAIA_TARGET_SEAT (3),
--      LEGAIA_FRAMES (3000), LEGAIA_PRESS_UNTIL (600), LEGAIA_TAIL (240).
--
-- Run:
--   LEGAIA_SPELL=0x86 bash scripts/pcsx-redux/run_probe.sh \
--       --scenario party_basic_attack_vs_gobu_gobu \
--       --lua scripts/pcsx-redux/autorun_audio_level_watch.lua --frames 3000
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3000)
local SPELL       = probe.getenv_num("LEGAIA_SPELL", 0x86)
local CASTER_SEAT = probe.getenv_num("LEGAIA_CASTER_SEAT", 0)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local PRESS_UNTIL = probe.getenv_num("LEGAIA_PRESS_UNTIL", 600)
local TAIL        = probe.getenv_num("LEGAIA_TAIL", 240)

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local B910        = 0x8007B910
local B6EC        = 0x8007B6EC
local SEQ_TABLE   = 0x801CD2C0

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function s32(a)
    local v = u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function ptr_ok(p) return p >= 0x80000000 and p < 0x80200000 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if not ptr_ok(p) then return nil end
    return p
end
local function seqvol(slot)
    local p = u32(SEQ_TABLE + slot * 4)
    if not ptr_ok(p) then return -1 end
    return u16(p + 0x58)
end

local writes, level
local vsync = -1
local injected, done_seen, done_vsync = false, false, -1
local last = { b910 = s32(B910), b6ec = s32(B6EC) }

local function on_write(word, addr)
    return function()
        local r = PCSX.getRegisters()
        local old = last[word]
        -- The breakpoint fires before the store retires: report the
        -- register about to land via the next vsync's read as `new`, and
        -- the pc / ra of the store now.
        writes:row("%d,%s,0x%08X,0x%08X,%d,%s", vsync, word, tou32(r.pc),
            tou32(r.GPR.n.ra), old, "next")
    end
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        writes = probe.csv_open(probe.out_path("writes.csv"), "vsync,word,pc,ra,old,new")
        level = probe.csv_open(probe.out_path("level.csv"),
            "vsync,st7,b910,b6ec,seqvol1,seqvol2,seqvol3")
        probe.arm_breakpoint(B6EC, "Write", 4, "b6ec", on_write("b6ec", B6EC))
        probe.arm_breakpoint(B910, "Write", 4, "b910", on_write("b910", B910))
        return {}
    end,

    on_capture = function(c, elapsed)
        vsync = elapsed
        probe.pad_release(probe.BTN.CROSS)
        local cxv = u32(CTX_PTR)
        local cx = ptr_ok(cxv) and cxv or nil
        local st7 = cx and u8(cx + 7) or -1
        if cx and not injected and elapsed < PRESS_UNTIL then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        if cx and not injected and st7 == 0x0A and u8(cx + 0x13) == CASTER_SEAT then
            local a = actor(CASTER_SEAT)
            if a ~= nil then
                injected = true
                probe.write_u16(a + 0x150, 999)
                probe.write_u8(a + 0x1DE, 2)
                probe.write_u8(a + 0x1DF, SPELL)
                probe.write_u8(a + 0x1DD, TARGET_SEAT)
                PCSX.log(string.format("[inject t%d] spell=0x%02X", elapsed, SPELL))
            end
        end
        last.b910, last.b6ec = s32(B910), s32(B6EC)
        level:row("%d,%d,%d,%d,%d,%d,%d", elapsed, st7, last.b910, last.b6ec,
            seqvol(1), seqvol(2), seqvol(3))
        if injected and not done_seen and (st7 == 0x50 or st7 == 0x51) then
            done_seen, done_vsync = true, elapsed
        end
        if done_seen and elapsed >= done_vsync + TAIL then c.request_quit = true end
    end,

    on_done = function()
        if writes then writes:close() end
        if level then level:close() end
        PCSX.quit(0)
    end,
})
