-- autorun_field_rand_callers.lua
--
-- Who draws from the BIOS rand() stream on a field state, vsync by vsync:
-- load a state, leave the pad alone, and count the calls of the SCUS rand
-- thunk FUN_80056798 by return address. The walk film's `rng` column says a
-- film parts from retail on a pass; this says which caller drew more or
-- fewer times there, against the engine's own per-consumer counts.
--
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--     --lua scripts/pcsx-redux/autorun_field_rand_callers.lua \
--     --scenario garmel_field_card_boot --frames 120 --out-dir <dir>
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR.
-- Output: rand_callers.csv (`vsync,ra,draws,seed`, one row per caller per
-- vsync that drew) and fog_pool.csv (`vsync,live,cap,recs`: the fog pool's
-- live count `_DAT_8007BCA8`, its cap `_DAT_8007BCB0` and every live record
-- as `slot:age:rate:x:z:y`, the rows `LEGAIA_RC_WALK_FOG=1` writes on the
-- engine side). Interpreter mode required (exec BP); not for --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 120)
local RAND = 0x80056798
local RAND_SEED = 0x80009010

local fh = assert(io.open(probe.out_path("rand_callers.csv"), "w"))
fh:write("vsync,ra,draws,seed\n")

local pool_fh = assert(io.open(probe.out_path("fog_pool.csv"), "w"))
pool_fh:write("vsync,live,cap,recs\n")
local FOG_POOL_PTR, FOG_LIVE, FOG_CAP = 0x8007B7E0, 0x8007BCA8, 0x8007BCB0

local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end
local function s32(v) if v >= 0x80000000 then return v - 0x100000000 end return v end

local function log_pool(el)
    local pool = probe.read_u32(FOG_POOL_PTR)
    local recs = {}
    if pool ~= nil and probe.in_ram(pool) then
        for i = 0, 0x4F do
            local r = pool + 0xA4 + i * 0x18
            if probe.read_u8(r + 5) ~= 0 then
                recs[#recs + 1] = string.format("%d:%d:%d:%d:%d:%d", probe.read_u8(r + 4),
                    probe.read_u16(r), probe.read_u16(r + 2), s32(probe.read_u32(r + 8)),
                    s32(probe.read_u32(r + 0xC)), s16(probe.read_u16(r + 0x10)))
            end
        end
    end
    pool_fh:write(string.format("%d,%d,%d,%s\n", el, probe.read_u16(FOG_LIVE),
        probe.read_u16(FOG_CAP), table.concat(recs, " ")))
    pool_fh:flush()
end

local counts, order = {}, {}

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_field_rand_callers.lua",
            { sstate = SSTATE, frames = FRAMES })
        probe.arm_breakpoint(RAND, "Exec", 4, "rand", function()
            local ra = PCSX.getRegisters().GPR.n.ra
            if ra < 0 then ra = ra + 0x100000000 end
            if counts[ra] == nil then
                counts[ra] = 0
                order[#order + 1] = ra
            end
            counts[ra] = counts[ra] + 1
        end)
        return {}
    end,
    on_capture = function(_, el)
        for _, ra in ipairs(order) do
            fh:write(string.format("%d,%08X,%d,%08x\n", el, ra, counts[ra],
                probe.read_u32(RAND_SEED)))
        end
        fh:flush()
        counts, order = {}, {}
        log_pool(el)
    end,
})
