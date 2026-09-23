-- autorun_w5a_0874_clobber.lua
--
-- Does the PROT 0874 loader's checksum gate actually fire, and what does
-- it guard? `FUN_8001E890` branches on `gp+0x6AC` (`0x8007B9C4`):
--
--   0 -> read the entry off the CD, decompress, register (`0x8001E910`)
--   2 -> re-sum the container and compare against `gp+0x6B8`
--        (`0x8007B9D0`); on a mismatch clear `gp+0x6AC` and `j 0x8001E900`,
--        i.e. go round again down the CD-read arm (`0x8001EA08`)
--   1 -> register only, no sum at all (`bne v1,v0` at `0x8001E974`)
--
-- and the sum arm's own first move is four `StoreImage` transfers
-- (`FUN_8005842C`, rects `(0x180 + 0x40*i, 0)` 0x40 x 0x100) that pull
-- 0x20000 bytes of VRAM back INTO the RAM buffer it is about to sum. So
-- the question "what breaks a rebuilt container" has three different
-- answers depending on which copy is damaged, and this probe drives all
-- three against a live retail run:
--
--   LEGAIA_CLOBBER=none  force the sum arm and change nothing - does an
--                        untouched pair compare equal?
--   LEGAIA_CLOBBER=ram   corrupt the RESIDENT RAM container before the
--                        read-back - the literal "clobber the container"
--                        test
--   LEGAIA_CLOBBER=sum   corrupt the stored sum word at `gp+0x6B8` just
--                        before the compare reads it
--
-- The sum arm only runs when `gp+0x6AC == 2`, which retail reaches from
-- PROT 0978's post-battle restore; a plain scene load leaves the word at
-- 0 (cold) or 1 (already registered). LEGAIA_FORCE_ENTRY=N forces the
-- word to 2 at the Nth entry to `FUN_8001E890` so a scene load reaches
-- the arm, which is what makes this runnable from a field anchor.
--
-- Launch (MUST be -interpreter -debugger; Lua BPs are dead under --fast):
--   LEGAIA_SSTATE=<a pre-scene-load state> LEGAIA_CLOBBER=sum \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_w5a_0874_clobber.lua --frames 600
--
-- Output: w5a_0874.csv (tick,site,detail...) + w5a_0874.log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local bit   = require("bit")

local GAME_MODE   = 0x8007B83C
local SCENE_NAME  = 0x8007050C
local GP_6AC      = 0x8007B9C4  -- load-state gate
local GP_6B8      = 0x8007B9D0  -- stored container sum
local FN_ENTRY    = 0x8001E890
local CD_READ_ARM = 0x8001E910  -- gate == 0 arm
local SUM_ARM     = 0x8001E97C  -- gate == 2 arm, before the StoreImage loop
local SUM_COMPARE = 0x8001E9F8  -- lw v0,0x6b8(gp); s2 holds the computed sum
local MISMATCH    = 0x8001EA08  -- sw zero,0x6ac(gp) - the reload arm
local JOIN        = 0x8001EA44  -- decompress / register join
local REGISTERED  = 0x8001EB0C  -- sw 1,0x6ac(gp)

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY  = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local POST_FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local MAX_TICKS   = probe.getenv_num("LEGAIA_MAX_TICKS", 3000)
local CLOBBER     = probe.getenv("LEGAIA_CLOBBER", "none")
local FORCE_ENTRY = probe.getenv_num("LEGAIA_FORCE_ENTRY", 1)

local CSV = probe.csv_open(probe.out_path("w5a_0874.csv"),
    "tick,site,gate,stored_sum,computed_sum,buf,mode,scene,note")
local LOGF = io.open(probe.out_path("w5a_0874.log"), "w")

local function log(s)
    PCSX.log("[w5a0874] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end

local function u8(a) return mem.read_u8(a) or 0 end
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function u32(a) return u32n(mem.read_u32(a) or 0) end
local function hex32(v) return string.format("0x%08X", u32n(v)) end

local function scene_name()
    local s = {}
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

local vsync, entries, done = 0, 0, false
local loaded_at = nil
local armed = false
local clobbered = false
local counts = {}

local function row(site, computed, buf, note)
    counts[site] = (counts[site] or 0) + 1
    CSV:row("%d,%s,%d,%s,%s,%s,0x%02X,%s,%s",
        vsync, site, u32(GP_6AC), hex32(u32(GP_6B8)),
        computed and hex32(computed) or "-", buf and hex32(buf) or "-",
        u8(GAME_MODE), scene_name(), note or "")
    CSV.fh:flush()
    log(string.format("%-12s tick=%d gate=%d stored=%s computed=%s buf=%s scene=%s %s",
        site, vsync, u32(GP_6AC), hex32(u32(GP_6B8)),
        computed and hex32(computed) or "-", buf and hex32(buf) or "-",
        scene_name(), note or ""))
end

local function regs() return PCSX.getRegisters() end

local function arm_all()
    bp.arm(FN_ENTRY, "Exec", 4, "entry", function()
        entries = entries + 1
        local note = ""
        if entries == FORCE_ENTRY and u32(GP_6AC) ~= 2 then
            mem.write_u32(GP_6AC, 2)
            note = "forced gate 0->2"
        end
        row("entry", nil, nil, note .. " (#" .. entries .. ")")
    end)
    bp.arm(CD_READ_ARM, "Exec", 4, "cd_read", function()
        row("cd_read_arm", nil, nil, "gate==0: reading PROT 0x36C")
    end)
    bp.arm(SUM_ARM, "Exec", 4, "sum_arm", function()
        local buf = u32n(tonumber(regs().GPR.n.s3))
        local note = "gate==2: StoreImage read-back then sum"
        if CLOBBER == "ram" and not clobbered then
            clobbered = true
            -- Corrupt the resident RAM container in four places spread over
            -- the read-back's 0x20000-byte window.
            for i = 0, 3 do
                local a = buf + i * 0x8000
                mem.write_u32(a, bit.bxor(u32(a), 0x5A5A5A5A))
            end
            note = note .. "; RAM container clobbered (4 words)"
        end
        row("sum_arm", nil, buf, note)
    end)
    bp.arm(SUM_COMPARE, "Exec", 4, "compare", function()
        local computed = u32n(tonumber(regs().GPR.n.s2))
        local note = ""
        if CLOBBER == "sum" and not clobbered then
            clobbered = true
            mem.write_u32(GP_6B8, bit.bxor(u32(GP_6B8), 1))
            note = "stored sum bit-flipped"
        end
        row("compare", computed, nil, note)
    end)
    bp.arm(MISMATCH, "Exec", 4, "mismatch", function()
        row("MISMATCH", nil, nil, "clearing gate + j 0x8001E900 (reload)")
    end)
    bp.arm(JOIN, "Exec", 4, "join", function()
        row("join", nil, nil, "decompress / register")
    end)
    bp.arm(REGISTERED, "Exec", 4, "registered", function()
        row("registered", nil, nil, "gate := 1")
    end)
    armed = true
    log(string.format("armed: clobber=%s force_entry=%d", CLOBBER, FORCE_ENTRY))
end

local function finish(why)
    if done then return end
    done = true
    log("--- " .. why .. " at tick " .. vsync .. " ---")
    for k, n in pairs(counts) do log(string.format("  %-12s %d", k, n)) end
    log(string.format("final gate=%d stored=%s scene=%s mode=0x%02X",
        u32(GP_6AC), hex32(u32(GP_6B8)), scene_name(), u8(GAME_MODE)))
    pcall(function() bp.disarm() end)
    CSV:close()
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local field_ticks = 0
local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded_at == nil then
        if SSTATE == "" then
            loaded_at = vsync
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load " .. SSTATE)
                finish("load failed")
                return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; mode=0x%02X scene=%s gate=%d",
                vsync, u8(GAME_MODE), scene_name(), u32(GP_6AC)))
        end
        return
    end
    if not armed then arm_all(); return end
    if u8(GAME_MODE) == 0x03 then
        field_ticks = field_ticks + 1
        if field_ticks >= POST_FRAMES then finish("post-field window done") end
    end
    if vsync >= MAX_TICKS then finish("max ticks") end
end

log("=== autorun_w5a_0874_clobber ===")
log(string.format("sstate=%s clobber=%s force_entry=%d",
    SSTATE == "" and "(none)" or SSTATE, CLOBBER, FORCE_ENTRY))

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
