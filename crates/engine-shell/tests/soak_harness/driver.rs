//! The soak driver: fan the runs out over worker threads, collect the
//! outcomes, and confirm a finding by replaying it.

use super::*;

// ---------------------------------------------------------------------------
// Soak driver
// ---------------------------------------------------------------------------

pub(super) struct SoakConfig {
    pub(super) tag: String,
    pub(super) scenes: Vec<String>,
    pub(super) seeds: Vec<u64>,
    pub(super) frames: u64,
    pub(super) jobs: usize,
    pub(super) confirm: bool,
}

pub(super) struct SoakResult {
    pub(super) outcomes: Vec<RunOutcome>,
    pub(super) hangs: Vec<Finding>,
    pub(super) wall: Duration,
}

pub(super) fn soak(src: &Source, known: &BTreeSet<String>, cfg: &SoakConfig) -> SoakResult {
    install_panic_hook();
    let t0 = Instant::now();
    let mut jobs: Vec<RunSpec> = Vec::new();
    for &seed in &cfg.seeds {
        for scene in &cfg.scenes {
            jobs.push(RunSpec {
                scene: scene.clone(),
                seed,
                frames: cfg.frames,
            });
        }
    }
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let board: StatusBoard = Arc::new(Mutex::new(vec![WorkerStatus::default(); cfg.jobs]));
    let results: Arc<Mutex<Vec<RunOutcome>>> = Arc::new(Mutex::new(Vec::new()));
    let hangs: Arc<Mutex<Vec<Finding>>> = Arc::new(Mutex::new(Vec::new()));
    let hang_secs = env_u64("LEGAIA_SOAK_HANG_SECS").unwrap_or(60);
    let out = out_dir(&cfg.tag);
    let _ = std::fs::create_dir_all(&out);

    // Watchdog: a single tick running past `hang_secs` cannot be interrupted,
    // so it is reported (with the spec that regenerates it) and the process
    // exits - a hung worker would otherwise hold the whole soak.
    let wd = {
        let board = board.clone();
        let done = done.clone();
        let out = out.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let b = board.lock().unwrap();
                for w in b.iter() {
                    if let Some(st) = w.tick_started
                        && st.elapsed() > Duration::from_secs(hang_secs)
                    {
                        let msg = format!(
                            "HANG: scene {} seed {} frame {}: one tick ran > {hang_secs}s\n\
                             reproduce: LEGAIA_SOAK_SCENES={} LEGAIA_SOAK_SEEDS={} \
                             LEGAIA_SOAK_FRAMES={}\n",
                            w.scene,
                            w.seed,
                            w.frame,
                            w.scene,
                            w.seed,
                            w.frame + 1
                        );
                        eprintln!("{msg}");
                        let _ = std::fs::write(out.join("HANG.txt"), &msg);
                        std::process::exit(3);
                    }
                }
            }
        })
    };

    let total = jobs.len();
    let mut handles = Vec::new();
    for wi in 0..cfg.jobs {
        let jobs = jobs.clone();
        let next = next.clone();
        let board = board.clone();
        let results = results.clone();
        let src = src.clone();
        let known = known.clone();
        let h = std::thread::Builder::new()
            .name(format!("soak-{wi}"))
            .stack_size(64 << 20)
            .spawn(move || {
                let t = Tunables::from_env();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(spec) = jobs.get(i) else { break };
                    {
                        let mut b = board.lock().unwrap();
                        b[wi] = WorkerStatus {
                            tick_started: None,
                            scene: spec.scene.clone(),
                            seed: spec.seed,
                            frame: 0,
                        };
                    }
                    let st = Instant::now();
                    let o = run_one(&src, &known, spec, None, &t, Some((&board, wi)));
                    eprintln!(
                        "[soak {}/{}] {:>10} seed {:<4} {:>6} fr {:>5.1}s {}{}",
                        i + 1,
                        total,
                        spec.scene,
                        spec.seed,
                        o.stats.frames_run,
                        st.elapsed().as_secs_f32(),
                        if o.findings.is_empty() { "ok" } else { "FIND" },
                        o.findings
                            .iter()
                            .map(|f| format!(" [{}]", f.signature()))
                            .collect::<String>()
                    );
                    results.lock().unwrap().push(o);
                }
            })
            .expect("spawn soak worker");
        handles.push(h);
    }
    for h in handles {
        let _ = h.join();
    }
    done.store(true, Ordering::Relaxed);
    let _ = wd.join();
    let mut outcomes = std::mem::take(&mut *results.lock().unwrap());
    outcomes.sort_by(|a, b| (a.spec.seed, &a.spec.scene).cmp(&(b.spec.seed, &b.spec.scene)));
    let hangs = std::mem::take(&mut *hangs.lock().unwrap());
    let r = SoakResult {
        outcomes,
        hangs,
        wall: t0.elapsed(),
    };
    write_report(src, known, cfg, &r);
    r
}

/// Confirm + minimise one finding: replay the recorded pads truncated to the
/// finding frame, report whether the same signature fires, and then zero out
/// chunks of the pad stream (a bounded delta-debugging pass) while it still
/// does. A softlock's reduction runs with the neutral-pad control
/// (`min_distinct = 1`), so pads that only fed the frozen window drop out and
/// what is left is the input that *caused* the park.
pub(super) fn confirm(
    src: &Source,
    known: &BTreeSet<String>,
    o: &RunOutcome,
    f: &Finding,
) -> (bool, Vec<u16>, Option<usize>) {
    let n = (f.frame + 1) as usize;
    let pads: Vec<u16> = o.pads[..n.min(o.pads.len())].to_vec();
    let (src, known, scene, seed, sig) = (
        src.clone(),
        known.clone(),
        o.spec.scene.clone(),
        o.spec.seed,
        f.signature(),
    );
    let arming = o.arming;
    let softlock = matches!(f.detector, "softlock");
    // A `control_starved` window is evidence only as the policy pressed it:
    // every way out a player would try, tried. Zeroing inside it leaves a
    // stream that confirms one looping picker option for the whole window,
    // which starves on a healthy conversation too. Only the pads ahead of
    // the window are reduced.
    let reducible = if f.detector == "control_starved" {
        pads.len()
            .saturating_sub(Tunables::from_env().starve_frames as usize + 1)
    } else {
        pads.len()
    };
    let budget = env_u64("LEGAIA_SOAK_MINIMIZE").unwrap_or(24) as usize;
    std::thread::Builder::new()
        .name("soak-confirm".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let reproduces = |pads: &[u16], relaxed: bool| -> bool {
                let mut t = Tunables::from_env();
                t.arming = arming;
                if relaxed {
                    t.min_distinct = 1;
                }
                let spec = RunSpec {
                    scene: scene.clone(),
                    seed,
                    frames: pads.len() as u64,
                };
                run_one(&src, &known, &spec, Some(pads), &t, None)
                    .findings
                    .iter()
                    .any(|g| g.signature() == sig)
            };
            if !reproduces(&pads, false) {
                return (false, pads, None);
            }
            let mut best = pads.clone();
            let mut tries = 0usize;
            let mut chunks = 2usize;
            while tries < budget && chunks <= reducible {
                let size = reducible.div_ceil(chunks);
                let mut changed = false;
                for c in 0..chunks {
                    if tries >= budget {
                        break;
                    }
                    let (a, b) = (c * size, ((c + 1) * size).min(reducible));
                    if a >= b || best[a..b].iter().all(|&p| p == 0) {
                        continue;
                    }
                    let mut cand = best.clone();
                    cand[a..b].iter_mut().for_each(|p| *p = 0);
                    tries += 1;
                    if reproduces(&cand, softlock) {
                        best = cand;
                        changed = true;
                    }
                }
                if !changed {
                    chunks *= 2;
                }
            }
            // A softlock whose pads were reduced replays under the relaxed
            // control it was reduced under.
            let relaxed = (softlock && best != pads).then_some(1);
            (true, best, relaxed)
        })
        .expect("spawn confirm")
        .join()
        .unwrap_or((false, Vec::new(), None))
}
