-- autorun_delilas_cast_e2e.lua
--
-- End-to-end repro for the Delilas-party Megaton Press freeze: every prior
-- cast probe started from a savestate whose battle was already loaded from
-- the RETAIL disc, so the restructured player battle file (staged cast rows
-- + descriptor push-up) never went through retail's real loader/rebaser
-- (FUN_80052FA0). This probe closes that gap:
--
--   1. load a plain FIELD savestate with the PATCHED disc mounted (--iso);
--   2. poke the patched disc's SCUS word-deltas into RAM (the savestate
--      carries vanilla SCUS) - poke file from a retail-vs-patched SCUS diff;
--   3. force a battle (formation cell + game_mode 8, the FUN_801DA51C
--      idiom) so the battle overlay, player packs and monster block all
--      stream from the PATCHED image through the real loader;
--   4. at battle main, dump the per-slot decoded record0 image bases and
--      the staged-row pointers/stream heads (rebase audit);
--   5. mash the command menu; whenever the cast slot's actor has an Attack
--      chosen (+0x1DE == 3), rewrite it to the exact bytes the shipped
--      queue hook writes (+0x1DE = 2, +0x1DF = spell, target kept but
--      defaulted if unset) - then Begin executes the capture cast;
--   6. log the cast phase walk (ctx+7 band, ctx+0x279 module phase, slot-B
--      module word) + screenshots; a hard freeze stops vsync callbacks, so
--      the log tail names the last live frame.
--
-- Env:
--   LEGAIA_SSTATE       field savestate (required)
--   LEGAIA_FRAMES       capture frames (default 6000)
--   LEGAIA_POKE_FILE    lua file returning { {va, word}, ... } (SCUS deltas)
--   LEGAIA_IDS          formation monster ids, comma list (default "10")
--   LEGAIA_PARTY        roster override bytes for 0x8007BD10 (default "1,2,3")
--   LEGAIA_FORCE_AT     frame to install the battle (default 150)
--   LEGAIA_CAST_SLOT    party actor slot to convert (default 2 = Gala seat)
--   LEGAIA_SPELL        spell id to install (default 0x7A = Megaton Press)
--   LEGAIA_TARGET_SEAT  target seat if the chosen one is not a monster (default 3)
--   LEGAIA_NO_CAST      1 = load-only run (no conversion; control arm)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local POKE_FILE = probe.getenv("LEGAIA_POKE_FILE", "")
local IDS_RAW = probe.getenv("LEGAIA_IDS", "10")
local PARTY_RAW = probe.getenv("LEGAIA_PARTY", "")
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 150)
local CAST_SLOT = probe.getenv_num("LEGAIA_CAST_SLOT", 2)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local NO_CAST = probe.getenv_num("LEGAIA_NO_CAST", 0)

local GAME_MODE = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local PARTY_TABLE = 0x8007BD10
local ACTOR_TABLE = 0x801C9370
local CTX_PTR = 0x8007BD24
local SLOTB = 0x801F69D8
local IMAGE_TABLE = 0x801C9360 -- per-party-slot decoded record0 image base
local BATTLE_MAIN = 0x15
local MALLOC_FAIL = 0x800178B8

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ptr_ok(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end
local function ctx()
    local c = u32(CTX_PTR)
    if not ptr_ok(c) then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if not ptr_ok(p) then return nil end
    return p
end

local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do ids[#ids + 1] = tonumber(tok) end
local party = {}
for tok in string.gmatch(PARTY_RAW, "[^,%s]+") do party[#party + 1] = tonumber(tok) end

local CSV = probe.csv_open(probe.out_path("delilas_cast_e2e.csv"),
    "tick,mode,phase,note")

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local poked = false
local installed = false
local main_at = nil
local audited = false
local converted = 0
local convert_logged = false
local cast_seen = false
local cast_logged = false
local last_mode = -1
local last_phase = -1
local shots = 0

local function row_audit()
    for slot = 0, 2 do
        local base = u32(IMAGE_TABLE + slot * 4)
        CSV:row("0,0,0,image[%d]=0x%08X", slot, base)
    end
    local base = u32(IMAGE_TABLE + CAST_SLOT * 4)
    if not ptr_ok(base) then
        CSV:row("0,0,0,AUDIT: cast slot image base invalid")
        return
    end
    for _, idx in ipairs({ 5, 6, 0x0A, 0x0B }) do
        local rp = u32(base + idx * 4)
        if ptr_ok(rp) then
            local parts = u8(rp + 0xAC)
            local frames = u8(rp + 0xAD)
            CSV:row("0,0,0,row[0x%02X]=0x%08X parts=%d frames=%d head=%02X%02X%02X%02X",
                idx, rp, parts, frames,
                u8(rp), u8(rp + 1), u8(rp + 2), u8(rp + 3))
        else
            CSV:row("0,0,0,row[0x%02X]=0x%08X INVALID", idx, u32(base + idx * 4))
        end
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function(c)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            local r = PCSX.getRegisters()
            local ok, s1 = pcall(function() return tonumber(r.GPR.n.s1) % 0x100000000 end)
            CSV:row("0,0,0,MALLOC FAILED size=0x%X", ok and s1 or 0)
        end)
        return {}
    end,
    on_capture = function(c, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1

        if elapsed == 60 and POKE_FILE ~= "" then
            local ok, list = pcall(dofile, POKE_FILE)
            if ok and type(list) == "table" then
                for _, e in ipairs(list) do
                    probe.write_u32(e[1], e[2])
                end
                poked = true
                CSV:row("%d,0x%X,0,poked %d SCUS words", elapsed, mode, #list)
            else
                CSV:row("%d,0x%X,0,POKE FILE FAILED: %s", elapsed, mode, tostring(list))
            end
        end

        if elapsed == FORCE_AT then
            if #party > 0 then
                for i = 0, 2 do probe.write_u8(PARTY_TABLE + i, party[i + 1] or 0) end
            end
            for i = 0, 3 do probe.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
            probe.write_u16(GAME_MODE, 8)
            installed = true
            CSV:row("%d,0x%X,0,installed ids=%s party=%s", elapsed, mode, IDS_RAW, PARTY_RAW)
            return
        end

        if mode ~= last_mode then
            CSV:row("%d,0x%X,0,mode-change", elapsed, mode)
            last_mode = mode
        end

        if installed and mode == BATTLE_MAIN then
            if main_at == nil then
                main_at = elapsed
                CSV:row("%d,0x%X,0,BATTLE MAIN", elapsed, mode)
            end
            local settle = elapsed - main_at
            if settle == 60 and not audited then
                audited = true
                row_audit()
                screenshot("battle_main")
            end

            -- mash the command flow
            local phase = settle % 40
            if phase == 0 then
                pad.force(pad.BTN.CROSS)
            elseif phase == 6 then
                pad.release(pad.BTN.CROSS)
            elseif phase == 20 then
                pad.force(pad.BTN.UP)
            elseif phase == 26 then
                pad.release(pad.BTN.UP)
            end

            -- The hook-equivalent conversion: force the staged action to
            -- the exact bytes the shipped queue hook writes (category 2,
            -- spell, target kept/defaulted), EVERY frame until the cast
            -- band opens - a one-member party begins the turn on the
            -- same frame the command is chosen, so a poll-for-3 misses
            -- the window.
            local cx0 = ctx()
            local ph0 = cx0 and u8(cx0 + 7) or -1
            local in_cast = (ph0 == 0x28) or (ph0 >= 0x6E and ph0 <= 0x71)
            if in_cast then cast_seen = true end
            if NO_CAST == 0 and settle > 60 and not cast_seen then
                local a = actor(CAST_SLOT)
                if a ~= nil then
                    local tgt = u8(a + 0x1DD)
                    if tgt < 3 or tgt > 6 then tgt = TARGET_SEAT end
                    probe.write_u8(a + 0x1DD, tgt)
                    probe.write_u8(a + 0x1DE, 2)
                    probe.write_u8(a + 0x1DF, SPELL)
                    converted = converted + 1
                    if not convert_logged then
                        convert_logged = true
                        CSV:row("%d,0x%X,0,FORCING slot=%d spell=0x%02X tgt=%d",
                            elapsed, mode, CAST_SLOT, SPELL, tgt)
                    end
                end
            end
            if cast_seen and not cast_logged then
                cast_logged = true
                CSV:row("%d,0x%X,0x%02X,CAST BAND ENTERED", elapsed, mode, ph0)
            end

            -- cast-phase watch
            local cx = ctx()
            local ph = cx and u8(cx + 7) or -1
            local mph = cx and u8(cx + 0x279) or -1
            if ph ~= last_phase then
                CSV:row("%d,0x%X,0x%02X,phase (modphase=0x%02X slotB=0x%08X conv=%d)",
                    elapsed, mode, ph, mph, u32(SLOTB), converted)
                last_phase = ph
            end
            if convert_logged and shots < 60 and settle % 30 == 0 then
                shots = shots + 1
                screenshot(string.format("cast_%05d_ph%02X", elapsed, ph))
            end
        end

        if elapsed % 300 == 0 then
            local cx = ctx()
            CSV:row("%d,0x%X,0x%02X,tick conv=%d", elapsed, mode,
                cx and u8(cx + 7) or -1, converted)
        end
    end,
    on_done = function()
        CSV:row("0,0,0,DONE poked=%s installed=%s main=%s converted=%d",
            tostring(poked), tostring(installed), tostring(main_at), converted)
        CSV:close()
    end,
})
