//! GTE (cop2) trace capture + replay harness.
//!
//! A *trace* is a deterministic record of the GTE register state before
//! and after each operation. Engines use traces to:
//!
//!   1. Validate the [`Gte`] emulator against captured retail RAM dumps -
//!      "after running RTPT with these registers, MAC1 should be X".
//!   2. Round-trip the emulator: run a sequence of ops, snapshot, replay,
//!      assert no divergence.
//!   3. Reproduce a captured cop2 stream in a deterministic test (the
//!      retail GTE has no random state; identical input → identical output).
//!
//! The harness is renderer-agnostic: the engine binary `gte-replay`
//! subcommand opens a trace file and reports per-step mismatches; tests
//! build synthetic traces to validate the round-trip.
//!
//! ## Trace format
//!
//! On disk a trace is the JSON serialisation of [`Cop2Trace`]. The JSON is
//! self-describing: each step carries the op, the input registers, and the
//! expected output registers. `Cop2Trace::write_json_pretty` and
//! `Cop2Trace::read_json` round-trip cleanly.

use crate::gte::{CopOp, Gte, GteMat3, GteVec3, NullMem, ScreenXY};
use serde::{Deserialize, Serialize};

/// Snapshot of the entire GTE register file. The `serde` derives let
/// engines persist traces to JSON for offline inspection / regression
/// suites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GteSnapshot {
    // Data registers.
    pub v0: [i32; 3],
    pub v1: [i32; 3],
    pub v2: [i32; 3],
    pub rgbc: [u8; 4],
    pub otz: u16,
    pub ir0: i32,
    pub ir1: i32,
    pub ir2: i32,
    pub ir3: i32,
    pub sxy_fifo: [(i32, i32); 3],
    pub sz_fifo: [u16; 4],
    pub rgb_fifo: [[u8; 4]; 3],
    pub mac0: i32,
    pub mac1: i64,
    pub mac2: i64,
    pub mac3: i64,
    pub flag: u32,

    // Control registers.
    pub rot: [[i16; 3]; 3],
    pub trans: [i32; 3],
    pub h: i32,
    pub ofx: i32,
    pub ofy: i32,
    pub zsf3: i32,
    pub zsf4: i32,
    pub dqa: i32,
    pub dqb: i32,
    pub light: [[i16; 3]; 3],
    pub light_color: [[i16; 3]; 3],
    pub back_color: [i32; 3],
    pub far_color: [i32; 3],

    // Cycle accumulator.
    pub cycles: u64,

    // Misc.
    pub lzcs: i32,
    pub res1: u32,
}

impl GteSnapshot {
    /// Capture a snapshot of `gte`'s current register state.
    pub fn capture(gte: &Gte) -> Self {
        Self {
            v0: vec3_to_array(gte.v[0]),
            v1: vec3_to_array(gte.v[1]),
            v2: vec3_to_array(gte.v[2]),
            rgbc: gte.rgbc,
            otz: gte.otz,
            ir0: gte.ir0,
            ir1: gte.ir1,
            ir2: gte.ir2,
            ir3: gte.ir3,
            sxy_fifo: [
                screen_to_pair(gte.sxy_fifo[0]),
                screen_to_pair(gte.sxy_fifo[1]),
                screen_to_pair(gte.sxy_fifo[2]),
            ],
            sz_fifo: gte.sz_fifo,
            rgb_fifo: gte.rgb_fifo,
            mac0: gte.mac0,
            mac1: gte.mac1,
            mac2: gte.mac2,
            mac3: gte.mac3,
            flag: gte.flag,
            rot: mat_to_array(gte.rot),
            trans: vec3_to_array(gte.trans),
            h: gte.h,
            ofx: gte.ofx,
            ofy: gte.ofy,
            zsf3: gte.zsf3,
            zsf4: gte.zsf4,
            dqa: gte.dqa,
            dqb: gte.dqb,
            light: mat_to_array(gte.light),
            light_color: mat_to_array(gte.light_color),
            back_color: vec3_to_array(gte.back_color),
            far_color: vec3_to_array(gte.far_color),
            cycles: gte.cycles,
            lzcs: gte.lzcs,
            res1: gte.res1,
        }
    }

    /// Restore this snapshot's state into `gte`. After calling, `gte`
    /// reads identical to a fresh capture of `self`.
    pub fn restore(&self, gte: &mut Gte) {
        gte.v[0] = vec3_from_array(self.v0);
        gte.v[1] = vec3_from_array(self.v1);
        gte.v[2] = vec3_from_array(self.v2);
        gte.rgbc = self.rgbc;
        gte.otz = self.otz;
        gte.ir0 = self.ir0;
        gte.ir1 = self.ir1;
        gte.ir2 = self.ir2;
        gte.ir3 = self.ir3;
        for i in 0..3 {
            gte.sxy_fifo[i] = ScreenXY {
                x: self.sxy_fifo[i].0,
                y: self.sxy_fifo[i].1,
            };
        }
        gte.sz_fifo = self.sz_fifo;
        gte.rgb_fifo = self.rgb_fifo;
        gte.mac0 = self.mac0;
        gte.mac1 = self.mac1;
        gte.mac2 = self.mac2;
        gte.mac3 = self.mac3;
        gte.flag = self.flag;
        gte.rot = mat_from_array(self.rot);
        gte.trans = vec3_from_array(self.trans);
        gte.h = self.h;
        gte.ofx = self.ofx;
        gte.ofy = self.ofy;
        gte.zsf3 = self.zsf3;
        gte.zsf4 = self.zsf4;
        gte.dqa = self.dqa;
        gte.dqb = self.dqb;
        gte.light = mat_from_array(self.light);
        gte.light_color = mat_from_array(self.light_color);
        gte.back_color = vec3_from_array(self.back_color);
        gte.far_color = vec3_from_array(self.far_color);
        gte.cycles = self.cycles;
        gte.lzcs = self.lzcs;
        gte.res1 = self.res1;
    }

    /// Compare this snapshot to `other`, returning per-field mismatches
    /// in render-friendly text form. An empty Vec means the snapshots are
    /// byte-identical.
    pub fn diff(&self, other: &Self) -> Vec<FieldMismatch> {
        let mut out = Vec::new();
        macro_rules! check {
            ($field:ident) => {
                if self.$field != other.$field {
                    out.push(FieldMismatch {
                        field: stringify!($field).to_string(),
                        expected: format!("{:?}", self.$field),
                        actual: format!("{:?}", other.$field),
                    });
                }
            };
        }
        check!(v0);
        check!(v1);
        check!(v2);
        check!(rgbc);
        check!(otz);
        check!(ir0);
        check!(ir1);
        check!(ir2);
        check!(ir3);
        check!(sxy_fifo);
        check!(sz_fifo);
        check!(rgb_fifo);
        check!(mac0);
        check!(mac1);
        check!(mac2);
        check!(mac3);
        check!(flag);
        check!(rot);
        check!(trans);
        check!(h);
        check!(ofx);
        check!(ofy);
        check!(zsf3);
        check!(zsf4);
        check!(dqa);
        check!(dqb);
        check!(light);
        check!(light_color);
        check!(back_color);
        check!(far_color);
        check!(cycles);
        check!(lzcs);
        check!(res1);
        out
    }
}

/// One per-field divergence between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMismatch {
    pub field: String,
    pub expected: String,
    pub actual: String,
}

fn vec3_to_array(v: GteVec3) -> [i32; 3] {
    [v.x, v.y, v.z]
}

fn vec3_from_array(a: [i32; 3]) -> GteVec3 {
    GteVec3 {
        x: a[0],
        y: a[1],
        z: a[2],
    }
}

fn mat_to_array(m: GteMat3) -> [[i16; 3]; 3] {
    m.m
}

fn mat_from_array(a: [[i16; 3]; 3]) -> GteMat3 {
    GteMat3 { m: a }
}

fn screen_to_pair(s: ScreenXY) -> (i32, i32) {
    (s.x, s.y)
}

/// One step of a recorded cop2 stream: an op plus before / after
/// snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    /// Display tag for the op. Stored as a string so JSON traces remain
    /// readable; `as_op()` decodes back into [`CopOp`].
    pub op: String,
    /// Snapshot before the op fires.
    pub before: GteSnapshot,
    /// Snapshot after the op finishes.
    pub after: GteSnapshot,
}

impl TraceStep {
    /// Try to parse `self.op` back to a [`CopOp`]. Returns `None` for
    /// strings outside the canonical set (custom op names - engines that
    /// use those skip via [`Cop2Trace::replay_skipping_unknown`]).
    pub fn as_op(&self) -> Option<CopOp> {
        match self.op.as_str() {
            "Rtps" => Some(CopOp::Rtps),
            "Nclip" => Some(CopOp::Nclip),
            "Op" => Some(CopOp::Op),
            "Dpcs" => Some(CopOp::Dpcs),
            "Intpl" => Some(CopOp::Intpl),
            "Mvmva" => Some(CopOp::Mvmva),
            "Ncds" => Some(CopOp::Ncds),
            "Cdp" => Some(CopOp::Cdp),
            "Ncdt" => Some(CopOp::Ncdt),
            "Nccs" => Some(CopOp::Nccs),
            "Cc" => Some(CopOp::Cc),
            "Ncs" => Some(CopOp::Ncs),
            "Nct" => Some(CopOp::Nct),
            "Sqr" => Some(CopOp::Sqr),
            "Dcpl" => Some(CopOp::Dcpl),
            "Dpct" => Some(CopOp::Dpct),
            "Avsz3" => Some(CopOp::Avsz3),
            "Avsz4" => Some(CopOp::Avsz4),
            "Rtpt" => Some(CopOp::Rtpt),
            "Gpf" => Some(CopOp::Gpf),
            "Gpl" => Some(CopOp::Gpl),
            "Ncct" => Some(CopOp::Ncct),
            _ => None,
        }
    }

    /// Encode `op` as the canonical lower-case-camel string used in trace
    /// files.
    pub fn op_name(op: CopOp) -> &'static str {
        match op {
            CopOp::Rtps => "Rtps",
            CopOp::Nclip => "Nclip",
            CopOp::Op => "Op",
            CopOp::Dpcs => "Dpcs",
            CopOp::Intpl => "Intpl",
            CopOp::Mvmva => "Mvmva",
            CopOp::Ncds => "Ncds",
            CopOp::Cdp => "Cdp",
            CopOp::Ncdt => "Ncdt",
            CopOp::Nccs => "Nccs",
            CopOp::Cc => "Cc",
            CopOp::Ncs => "Ncs",
            CopOp::Nct => "Nct",
            CopOp::Sqr => "Sqr",
            CopOp::Dcpl => "Dcpl",
            CopOp::Dpct => "Dpct",
            CopOp::Avsz3 => "Avsz3",
            CopOp::Avsz4 => "Avsz4",
            CopOp::Rtpt => "Rtpt",
            CopOp::Gpf => "Gpf",
            CopOp::Gpl => "Gpl",
            CopOp::Ncct => "Ncct",
        }
    }
}

/// One mismatch result from [`Cop2Trace::replay`].
#[derive(Debug, Clone)]
pub struct StepMismatch {
    /// Step index in the trace.
    pub step: usize,
    /// Op name as recorded.
    pub op: String,
    /// Per-field divergence.
    pub fields: Vec<FieldMismatch>,
}

/// Cop2 instruction trace. A linear sequence of [`TraceStep`]s.
///
/// Traces can be:
///   - Constructed live with [`TraceRecorder`].
///   - Loaded from JSON via [`Cop2Trace::read_json`].
///   - Replayed against a fresh [`Gte`] via [`Cop2Trace::replay`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cop2Trace {
    /// Optional human-readable label for the trace (the originating scene,
    /// frame number, etc.). Engines stash provenance info here.
    pub label: Option<String>,
    /// The sequence of recorded ops.
    pub steps: Vec<TraceStep>,
}

impl Cop2Trace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            steps: Vec::new(),
        }
    }

    /// Number of recorded steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Replay every step against a fresh [`Gte`], comparing the resulting
    /// snapshot to each step's recorded `after`. Returns the list of
    /// divergences (empty when the trace replays bit-exact).
    pub fn replay(&self) -> Vec<StepMismatch> {
        let mut out = Vec::new();
        let mut gte = Gte::new();
        for (idx, step) in self.steps.iter().enumerate() {
            // Restore the recorded "before" state.
            step.before.restore(&mut gte);
            // Re-run the op.
            let Some(op) = step.as_op() else {
                // Unknown op name - skip this step rather than fail.
                continue;
            };
            // The op itself charges cycles via `begin_op`; no extra
            // charge needed here.
            run_op(&mut gte, op);
            // Compare against expected.
            let actual = GteSnapshot::capture(&gte);
            let fields = step.after.diff(&actual);
            if !fields.is_empty() {
                out.push(StepMismatch {
                    step: idx,
                    op: step.op.clone(),
                    fields,
                });
            }
        }
        out
    }

    /// Replay strict - replays every step and panics on the first divergence.
    /// Useful for tests where the expected outcome is "no mismatches at all".
    pub fn replay_strict(&self) -> Result<(), StepMismatch> {
        match self.replay().into_iter().next() {
            Some(m) => Err(m),
            None => Ok(()),
        }
    }

    /// Pretty-print the trace as JSON.
    pub fn write_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Cop2Trace is always serialisable")
    }

    /// Parse a trace from JSON.
    pub fn read_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Append a step to the trace.
    pub fn push(&mut self, step: TraceStep) {
        self.steps.push(step);
    }
}

/// Helper for live trace recording: wrap a [`Gte`], call methods through
/// the recorder, and the before/after snapshots are pushed automatically.
///
/// Engines that already drive a `Gte` directly can just snapshot before
/// + after each op manually; the recorder is for high-level use.
pub struct TraceRecorder {
    pub gte: Gte,
    pub trace: Cop2Trace,
}

impl Default for TraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceRecorder {
    pub fn new() -> Self {
        Self {
            gte: Gte::new(),
            trace: Cop2Trace::default(),
        }
    }

    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            gte: Gte::new(),
            trace: Cop2Trace::with_label(label),
        }
    }

    /// Mutable handle on the underlying `Gte` for setup (loading rotation
    /// matrices, vertices, etc.) before the first `record` call.
    pub fn gte_mut(&mut self) -> &mut Gte {
        &mut self.gte
    }

    /// Run `op` against the inner GTE and append a [`TraceStep`] to the
    /// trace.
    ///
    /// Each public op already charges its own cycle count via the
    /// `Gte::begin_op` helper, so the recorder doesn't double-charge.
    pub fn record(&mut self, op: CopOp) {
        let before = GteSnapshot::capture(&self.gte);
        run_op(&mut self.gte, op);
        let after = GteSnapshot::capture(&self.gte);
        self.trace.push(TraceStep {
            op: TraceStep::op_name(op).to_string(),
            before,
            after,
        });
    }

    /// Consume the recorder, returning the populated trace.
    pub fn into_trace(self) -> Cop2Trace {
        self.trace
    }
}

/// Run `op` against `gte`. Centralised dispatcher used by both the
/// recorder and the replayer so the two paths are guaranteed to be
/// identical.
fn run_op(gte: &mut Gte, op: CopOp) {
    // The cycle counter is charged separately so each call site (recorder
    // / replayer) can decide whether to track it.
    let _ = gte; // silence unused-mut on no-op branches below.
    match op {
        CopOp::Rtps => {
            gte.rtps();
        }
        CopOp::Nclip => {
            gte.nclip();
        }
        CopOp::Op => {
            gte.op(true);
        }
        CopOp::Dpcs => {
            gte.dpcs();
        }
        CopOp::Intpl => {
            gte.intpl();
        }
        CopOp::Mvmva => {
            // MVMVA needs explicit selection bits - engines that record
            // an MVMVA step should manually push the step rather than rely
            // on the recorder's defaults. We use (rot, V0, trans, true,
            // true) as the canonical "RT * V0 + TR with shift+lm" form.
            let rot = gte.rot;
            let v = gte.v[0];
            let trans = gte.trans;
            gte.mvmva(&rot, v, trans, true, true);
        }
        CopOp::Ncds => {
            gte.ncds();
        }
        CopOp::Cdp => {
            gte.cdp();
        }
        CopOp::Ncdt => {
            gte.ncdt();
        }
        CopOp::Nccs => {
            gte.nccs();
        }
        CopOp::Cc => {
            gte.cc();
        }
        CopOp::Ncs => {
            gte.ncs();
        }
        CopOp::Nct => {
            gte.nct();
        }
        CopOp::Sqr => {
            gte.sqr(true);
        }
        CopOp::Dcpl => {
            gte.dcpl();
        }
        CopOp::Dpct => {
            gte.dpct();
        }
        CopOp::Avsz3 => {
            gte.avsz3();
        }
        CopOp::Avsz4 => {
            gte.avsz4();
        }
        CopOp::Rtpt => {
            gte.rtpt();
        }
        CopOp::Gpf => {
            gte.gpf(true);
        }
        CopOp::Gpl => {
            gte.gpl(true);
        }
        CopOp::Ncct => {
            gte.ncct();
        }
    }
    let _ = NullMem; // ensure unused-import warning suppressed.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gte::{Gte, GteMat3, GteVec3};

    fn priming_gte() -> Gte {
        let mut g = Gte::new();
        g.set_viewport(320, 240);
        g.h = 200;
        // Set up an identity rotation + a known vertex so RTPS produces
        // determinate values.
        g.rot = GteMat3::IDENTITY;
        g.trans = GteVec3 { x: 0, y: 0, z: 100 };
        g.v[0] = GteVec3 { x: 50, y: 25, z: 0 };
        g.v[1] = GteVec3 {
            x: -10,
            y: 5,
            z: 30,
        };
        g.v[2] = GteVec3 { x: 0, y: 0, z: 60 };
        g
    }

    #[test]
    fn snapshot_capture_round_trips_through_restore() {
        let mut g = priming_gte();
        g.rtps();
        let snap = GteSnapshot::capture(&g);
        let mut g2 = Gte::new();
        snap.restore(&mut g2);
        let snap2 = GteSnapshot::capture(&g2);
        assert_eq!(snap, snap2);
    }

    #[test]
    fn snapshot_diff_returns_empty_when_identical() {
        let g = priming_gte();
        let a = GteSnapshot::capture(&g);
        let b = a.clone();
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn snapshot_diff_surfaces_per_field_mismatch() {
        let g = priming_gte();
        let mut a = GteSnapshot::capture(&g);
        let mut b = a.clone();
        b.mac0 = 12345;
        let diffs = a.diff(&b);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "mac0");

        // Multiple-field divergence.
        b.mac1 = 99;
        a.mac0 = -7;
        let diffs = a.diff(&b);
        assert!(diffs.iter().any(|d| d.field == "mac0"));
        assert!(diffs.iter().any(|d| d.field == "mac1"));
    }

    #[test]
    fn recorder_round_trip_replays_without_mismatches() {
        let mut rec = TraceRecorder::with_label("rtps_smoke");
        // Prime the GTE.
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtps);
        rec.record(CopOp::Nclip);
        let trace = rec.into_trace();
        assert_eq!(trace.len(), 2);
        let mismatches = trace.replay();
        assert!(
            mismatches.is_empty(),
            "round trip produced mismatches: {:?}",
            mismatches
        );
    }

    #[test]
    fn replay_detects_corrupted_step() {
        let mut rec = TraceRecorder::new();
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtps);
        let mut trace = rec.into_trace();
        // Corrupt step 0's expected MAC1 to provoke a mismatch.
        trace.steps[0].after.mac1 += 1234;
        let mismatches = trace.replay();
        assert_eq!(mismatches.len(), 1);
        assert!(mismatches[0].fields.iter().any(|f| f.field == "mac1"));
    }

    #[test]
    fn replay_strict_returns_first_mismatch() {
        let mut rec = TraceRecorder::new();
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtps);
        rec.record(CopOp::Avsz3);
        let mut trace = rec.into_trace();
        // Corrupt step 1.
        trace.steps[1].after.otz = 0xDEAD;
        let result = trace.replay_strict();
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.step, 1);
    }

    #[test]
    fn json_round_trip_preserves_trace() {
        let mut rec = TraceRecorder::with_label("json_test");
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtps);
        rec.record(CopOp::Nclip);
        let trace = rec.into_trace();
        let json = trace.write_json_pretty();
        let parsed = Cop2Trace::read_json(&json).expect("parse");
        assert_eq!(parsed.len(), trace.len());
        assert_eq!(parsed.label, Some("json_test".into()));
        // Replay the parsed trace; it must still be valid.
        let mismatches = parsed.replay();
        assert!(mismatches.is_empty());
    }

    #[test]
    fn op_name_round_trips_through_as_op() {
        let ops = [
            CopOp::Rtps,
            CopOp::Nclip,
            CopOp::Mvmva,
            CopOp::Avsz3,
            CopOp::Avsz4,
            CopOp::Rtpt,
            CopOp::Ncds,
            CopOp::Ncct,
        ];
        for op in ops {
            let name = TraceStep::op_name(op);
            let step = TraceStep {
                op: name.into(),
                before: GteSnapshot::capture(&Gte::new()),
                after: GteSnapshot::capture(&Gte::new()),
            };
            assert_eq!(step.as_op(), Some(op));
        }
    }

    #[test]
    fn unknown_op_string_returns_none_from_as_op() {
        let step = TraceStep {
            op: "DefinitelyNotARealOp".into(),
            before: GteSnapshot::capture(&Gte::new()),
            after: GteSnapshot::capture(&Gte::new()),
        };
        assert!(step.as_op().is_none());
    }

    #[test]
    fn replay_skips_unknown_ops_silently() {
        let mut rec = TraceRecorder::new();
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtps);
        let mut trace = rec.into_trace();
        // Inject a step with an unknown op name; replay should skip it.
        trace.steps.push(TraceStep {
            op: "SyntheticTest".into(),
            before: GteSnapshot::capture(&Gte::new()),
            after: GteSnapshot::capture(&Gte::new()),
        });
        let mismatches = trace.replay();
        assert!(mismatches.is_empty());
    }

    #[test]
    fn cycle_counter_advances_per_recorded_op() {
        let mut rec = TraceRecorder::new();
        *rec.gte_mut() = priming_gte();
        let cycles_before = rec.gte.cycles;
        rec.record(CopOp::Rtps);
        assert_eq!(rec.gte.cycles, cycles_before + CopOp::Rtps.cycles() as u64);
    }

    #[test]
    fn empty_trace_replays_with_no_mismatches() {
        let trace = Cop2Trace::new();
        assert!(trace.is_empty());
        assert!(trace.replay().is_empty());
    }

    #[test]
    fn rtpt_batch_round_trips() {
        let mut rec = TraceRecorder::with_label("rtpt_batch");
        *rec.gte_mut() = priming_gte();
        rec.record(CopOp::Rtpt);
        rec.record(CopOp::Avsz3);
        let trace = rec.into_trace();
        assert_eq!(trace.len(), 2);
        assert!(trace.replay().is_empty());
    }

    #[test]
    fn label_persists_through_json_round_trip() {
        let trace = Cop2Trace::with_label("scene_dolk_frame_42");
        let json = trace.write_json_pretty();
        let parsed = Cop2Trace::read_json(&json).unwrap();
        assert_eq!(parsed.label.as_deref(), Some("scene_dolk_frame_42"));
    }
}
