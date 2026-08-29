-- From a field save with Noa in the party, set Noa's equipped weapon
-- to 0xBA (Astral Sword) in her character record, force an encounter the way
-- autorun_delilas_battle_load.lua does, mash an attack, and screenshot the
-- battle every SHOT_EVERY frames.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local BATTLE_MAIN    = 0x15
local CHAR_REC       = 0x80084708
local REC_STRIDE     = 0x414
-- Party slot whose record gets the weapon (0 Vahn, 1 Noa, 2 Gala) and the
-- equipment byte inside it: section 3 (+0x199) is Noa's weapon, section 2
-- (+0x198) is Vahn's / Gala's.
local NOA            = tonumber(probe.getenv("LEGAIA_CHAR", "1"))
local EQUIP_OFF      = tonumber(probe.getenv("LEGAIA_EQUIP_OFF", "0x199"))

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1400)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 60)
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 60)
local WEAPON   = probe.getenv_num("LEGAIA_WEAPON", 0xBA)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "4")
local TAG      = probe.getenv("LEGAIA_TAG", "run")
local NO_FORCE = probe.getenv("LEGAIA_NO_FORCE", "") ~= ""
-- The equipment byte is only re-read when the player files stream, and
-- they stream on a SCENE load (world map / field entry), never on a battle
-- start - a forced encounter reuses the resident party meshes. So the
-- write goes in early (WRITE_AT), a Down press walks the state through its
-- scene transition (PRESS_DOWN_AT / PRESS_DOWN_FRAMES; the Octam save warps
-- to the Sebucus world map), and the encounter is forced after the new
-- scene's player files have streamed (FORCE_AT).
local WRITE_AT = probe.getenv_num("LEGAIA_WRITE_AT", FORCE_AT)
local PRESS_DOWN_AT = probe.getenv_num("LEGAIA_PRESS_DOWN_AT", -1)
local PRESS_DOWN_FRAMES = probe.getenv_num("LEGAIA_PRESS_DOWN_FRAMES", 70)
local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do ids[#ids + 1] = tonumber(tok) end

local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(probe.out_path(name .. ".raw"), "wb"); h:write(tostring(ss.data)); h:close()
        local m = io.open(probe.out_path(name .. ".meta"), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp)); m:close()
    end
end

local installed, settle, last_mode = false, 0, -1
local bp_hits, seek_hits = 0, 0
local settle_t = 0
local reverted = false
probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        -- Prove the player files stream during THIS battle and where their
        -- slots are sought: the TOC resolver (`FUN_8003E8A8`, a0 = raw TOC
        -- index; 0x361..0x364 are PLAYER1..4) and the forward slot seek
        -- (`FUN_8003E964`, a0 = sectors to skip - an annexed file's first
        -- seek jumps to DMY.DAT, ~150k sectors).
        local function a0()
            local r = PCSX.getRegisters()
            return tonumber(r.GPR and r.GPR.n and r.GPR.n.a0 or 0) or 0
        end
        probe.arm_breakpoint(0x8003E8A8, "Exec", 4, "toc_resolve", function()
            local idx = a0()
            bp_hits = bp_hits + 1
            if bp_hits <= 8 then PCSX.log(string.format("[lane-c] toc resolve #%d idx=0x%X", bp_hits, idx)) end
            if idx >= 0x361 and idx <= 0x364 then
                PCSX.log(string.format("[lane-c] t%d player file open: toc index 0x%X (PLAYER%d)", settle_t, idx, idx - 0x360))
            end
        end)
        probe.arm_breakpoint(0x8003E964, "Exec", 4, "slot_seek", function()
            local n = a0()
            seek_hits = seek_hits + 1
            if n >= 0x800 then
                PCSX.log(string.format("[lane-c] slot seek +%d sectors%s", n, n >= 0x10000 and " (into DMY.DAT)" or ""))
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        settle_t = elapsed
        local mode = probe.read_u16(GAME_MODE) or -1
        if PRESS_DOWN_AT >= 0 then
            if elapsed == PRESS_DOWN_AT then pad.force(pad.BTN.DOWN) end
            if elapsed == PRESS_DOWN_AT + PRESS_DOWN_FRAMES then pad.release(pad.BTN.DOWN) end
        end
        if elapsed == WRITE_AT then
            local rec = CHAR_REC + NOA * REC_STRIDE
            local name = ""
            for i = 0, 7 do local c = probe.read_u8(rec + 0x2A7 + i) or 0; if c == 0 then break end; name = name .. string.char(c) end
            local eq = {}
            for i = 0, 4 do eq[#eq+1] = string.format("%02X", probe.read_u8(rec + 0x196 + i) or 0) end
            PCSX.log(string.format("[lane-c] mode=0x%X party=%d ids=%02X %02X %02X noa-rec name='%s' equip=%s",
                mode, probe.read_u8(0x80084594) or 0, probe.read_u8(0x80084598) or 0, probe.read_u8(0x80084599) or 0, probe.read_u8(0x8008459A) or 0, name, table.concat(eq, " ")))
            probe.write_u8(rec + EQUIP_OFF, WEAPON)
            PCSX.log(string.format("[lane-c] wrote weapon byte +0x%X = 0x%02X (now %02X)", EQUIP_OFF, WEAPON, probe.read_u8(rec + EQUIP_OFF) or 0))
            if WRITE_AT ~= FORCE_AT then return end
        end
        if elapsed == FORCE_AT then
            -- A forced encounter (mode 8) never re-streams the player files:
            -- the assembled party meshes stay resident from the last scene
            -- load, so a look change is invisible to it. LEGAIA_NO_FORCE=1
            -- leaves the state's own scripted battle to fire, which does
            -- stream them (the Cort pre-battle save).
            if not NO_FORCE then
                for i = 0, 3 do probe.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
                probe.write_u16(GAME_MODE, 8)
            end
            installed = true
            return
        end
        if mode ~= last_mode then PCSX.log(string.format("[lane-c] t%d mode 0x%X", elapsed, mode)); last_mode = mode end
        if installed then
            local now = probe.read_u8(CHAR_REC + NOA * REC_STRIDE + EQUIP_OFF) or 0
            if now ~= WEAPON and not reverted then
                reverted = true
                PCSX.log(string.format("[lane-c] t%d weapon byte reverted to 0x%02X (mode 0x%X)", elapsed, now, mode))
            end
        end
        if installed and mode == BATTLE_MAIN then
            settle = settle + 1
            if settle % SHOT_EVERY == 1 then shot(string.format("%s_s%04d", TAG, settle)) end
            local phase = settle % 40
            if phase == 0 then pad.force(pad.BTN.CROSS)
            elseif phase == 6 then pad.release(pad.BTN.CROSS)
            elseif phase == 20 then pad.force(pad.BTN.UP)
            elseif phase == 26 then pad.release(pad.BTN.UP) end
            if settle >= FRAMES - FORCE_AT - 300 then ctx.request_quit = true end
        end
    end,
    on_done = function() PCSX.log(string.format("[lane-c] done settle=%d last_mode=0x%X toc_hits=%d seek_hits=%d", settle, last_mode, bp_hits, seek_hits)) end,
})
