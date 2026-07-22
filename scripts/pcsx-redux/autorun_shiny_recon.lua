-- autorun_shiny_recon.lua
--
-- Runtime recon for the shiny-Seru randomizer feature (`legaia_patcher::shiny_seru`).
-- Confirms, in live RAM, the things the disc patch depends on, and observes its
-- two write-side routines firing. Run it against the BOOTED PATCHED DISC
-- (`legaia_shiny_100.bin`) - or against a vanilla savestate to confirm the patch
-- *targets* are correct before patching.
--
-- It reports two things:
--
--   1. Patch presence (static, immediate). Each of the 8 hook sites is either the
--      original instruction (vanilla) or a `j` (patched, op 0x02); and the new
--      SCUS rodata gap at 0x80077728 is either all-zero (vanilla dead space) or
--      carries the injected routines (patched). So one glance says "am I on the
--      patched disc, and did it land in RAM".
--
--   2. The routines firing (live, while you play). In any battle it scans the
--      monster slots for the shiny marker the SETUP routine writes (+0x226 != 0)
--      and reports that enemy's capturable-Seru id (+0x3e), boosted ATK (+0x158)
--      and HP (+0x172); and it scans the party records' Seru-magic level bytes
--      (record+0x161) for the high bit 0x80 the GRANT routine sets when you
--      capture a shiny Seru. Seeing either is direct proof the injected code ran.
--
-- No input, no poking; polls a window and quits. The +35% *damage* is read-side
-- (only visible during an actual Seru cast) - that you confirm in-game: a
-- captured shiny Seru's spell hits ~35% harder.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local GMODE       = 0x8007B83C
local CHAR_BASE   = 0x80084708 -- live character record base; +slot*0x414
local CHAR_STRIDE = 0x414
local SCUS_GAP    = 0x80077728 -- the new shiny rodata gap (scratch + routines)
local LEVEL_OFF   = 0x161      -- spell-level array in the record (high bit 0x80 = shiny)

-- (label, hook VA, original first word) for the 8 detour sites.
local HOOKS = {
    { "setup     ", 0x80051A20, 0x3C028008 },
    { "capture   ", 0x801EE2E8, 0xA0820269 },
    { "grant     ", 0x801E93B4, 0xA0430729 },
    { "damage    ", 0x801DDB08, 0x90420729 },
    { "lvl-gate  ", 0x801E71C8, 0x90C20729 },
    { "lvl-read  ", 0x801E71DC, 0x90C70729 },
    { "lvl-write ", 0x801E7224, 0xA0C20729 },
    { "menu      ", 0x801D2FA0, 0x8C6346B0 },
}

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end

local function patch_check()
    -- The SCUS gap: all-zero on vanilla, routine bytes on the patched disc.
    local nonzero = 0
    for i = 0, 60, 4 do if u32(SCUS_GAP + i) ~= 0 then nonzero = nonzero + 1 end end
    PCSX.log(string.format(
        "== shiny recon: PATCH PRESENCE ==  SCUS gap 0x%08X: %s",
        SCUS_GAP, (nonzero > 0) and "carries routines (PATCHED)" or "all-zero (vanilla)"))
    for _, h in ipairs(HOOKS) do
        local w = u32(h[2])
        local op = math.floor(w / 0x4000000)
        local state
        if op == 0x02 then state = "j routine  (PATCHED)"
        elseif w == h[3] then state = "original   (vanilla)"
        else state = "UNEXPECTED" end
        PCSX.log(string.format("  hook %s 0x%08X = 0x%08X  %s", h[1], h[2], w, state))
    end
end

local last_shiny_enemy = -1
local last_shiny_spell = ""

local function scan_battle()
    local pc = ok_ptr(u32(CTX_PTR)) and u8(u32(CTX_PTR) + 0) or 0
    -- monster slots are pc..6; scan 3..6 (worst case) for the shiny marker.
    for slot = math.max(pc, 3), 6 do
        local p = u32(ACTOR_TABLE + slot * 4)
        if ok_ptr(p) then
            local marker = u8(p + 0x226)
            if marker ~= 0 and slot ~= last_shiny_enemy then
                last_shiny_enemy = slot
                PCSX.log(string.format(
                    ">> SHINY enemy: slot %d  capturable_seru(+0x3e)=0x%02X  marker(+0x226)=%d  "
                    .. "ATK(+0x158)=%d  HP(+0x172)=%d  (setup routine fired)",
                    slot, u8(p + 0x3e), marker, s16(u16(p + 0x158)), s16(u16(p + 0x172))))
            end
        end
    end
end

local function scan_shiny_spells()
    for slot = 0, 3 do
        local base = CHAR_BASE + slot * CHAR_STRIDE
        if ok_ptr(base) then
            for i = 0, 11 do
                local lv = u8(base + LEVEL_OFF + i)
                if lv >= 0x80 then
                    local key = string.format("c%d.s%d", slot, i)
                    if key ~= last_shiny_spell then
                        last_shiny_spell = key
                        PCSX.log(string.format(
                            ">> SHINY spell: char %d, spell slot %d  level byte=0x%02X "
                            .. "(real level %d + shiny bit; grant routine fired)",
                            slot, i, lv, lv % 0x80))
                    end
                end
            end
        end
    end
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = 1800, -- ~30s window to fight / capture while it watches
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed == 30 then patch_check() end
        if elapsed >= 60 then
            scan_battle()
            scan_shiny_spells()
        end
    end,
})
