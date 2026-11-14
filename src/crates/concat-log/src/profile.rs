// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah
//! Where the time went, measured while it goes.
//!
//! A log says what happened. This says what it *cost*: every named piece of
//! work is timed where it runs, on the thread that ran it, and once a second
//! one line per scope says how many times it ran and what each run cost.
//!
//! ```text
//! profile 1.02 s · 14 frames shown, 14 requested, 0 dropped
//!   main     publish.lanes       840 × 0.42 ms avg · p95 0.9 · max 2.1 · 34.6% of the second
//!   worker-2 preview.decode        14 × 31.2 ms avg · p95 60.1 · max 71.0
//!   main     preview.show          14 × 6.10 ms avg · p95 8.2 · max 11.0 · 8.4%
//! ```
//!
//! That is the difference between "the preview is slow" and "one decode on
//! one worker takes 31 ms and the UI thread spends a third of every second
//! rebuilding the timeline".
//!
//! # Turning it on
//!
//! Off unless `CONCAT_PROFILE` is set, so a normal run pays one relaxed
//! atomic load per span and nothing else:
//!
//! - `CONCAT_PROFILE=1` - the table once a second, at info.
//! - `CONCAT_PROFILE=frames` - the table, and every frame that took over
//!   [`SLOW_FRAME`] as its own line with the pieces named.
//!
//! # What it costs
//!
//! A span is an `Instant::now` at the start and one at the end, plus a lock
//! on a small map when it ends. Scopes are named with `&'static str`, so
//! nothing is allocated per call. It is not free - a scope per pixel would
//! show up - but at tens of spans a frame it is under a percent.
//!
//! # Reading it
//!
//! `% of the second` is only shown for scopes on a thread that was working
//! the whole second (the UI thread); it is what tells a scope that is
//! *called often* from a scope that is *slow*. `p95` is the number to look
//! at for a frame budget: the average hides the frames that hitch.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A frame longer than this is worth its own line; see the module.
pub const SLOW_FRAME: Duration = Duration::from_millis(50);

/// Whether `CONCAT_PROFILE` asked for the table, and whether it asked for
/// per-frame lines too.
static ON: AtomicBool = AtomicBool::new(false);
static PER_FRAME: AtomicBool = AtomicBool::new(false);
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Whether anything is being recorded. False makes [`Span::new`] as cheap as
/// an `Option` that is never filled.
#[inline]
pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Whether slow frames get a line of their own.
#[inline]
pub fn frames_enabled() -> bool {
    PER_FRAME.load(Ordering::Relaxed)
}

/// Starts the profiler if the environment asks, and the thread that writes
/// the table. Called once, as the app starts; safe to call again.
pub fn start() {
    let asked = std::env::var("CONCAT_PROFILE").unwrap_or_default();
    let asked = asked.trim();
    if asked.is_empty() || asked == "0" || asked.eq_ignore_ascii_case("off") {
        return;
    }
    if REPORTED.swap(true, Ordering::SeqCst) {
        return;
    }
    ON.store(true, Ordering::Relaxed);
    let per_frame = asked.eq_ignore_ascii_case("frames") || asked.eq_ignore_ascii_case("all");
    PER_FRAME.store(per_frame, Ordering::Relaxed);
    std::thread::Builder::new()
        .name("concat-profile".to_owned())
        .spawn(|| {
            let mut last = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                let now = Instant::now();
                report(now.duration_since(last));
                last = now;
            }
        })
        .ok();
    log::info!(
        "profile: on{} - a table a second · the run costs a little more for it",
        if per_frame { " with per-frame lines" } else { "" }
    );
}

/// One piece of work being timed. Dropped at the end of the block it was
/// opened in:
///
/// ```
/// let _t = concat_log::profile::Span::new("preview.decode");
/// // ... the decode ...
/// ```
#[must_use = "a span records nothing if it is dropped at once"]
pub struct Span {
    scope: &'static str,
    at: Option<Instant>,
}

impl Span {
    #[inline]
    pub fn new(scope: &'static str) -> Self {
        Span {
            scope,
            at: enabled().then(Instant::now),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(at) = self.at {
            record(self.scope, at.elapsed());
        }
    }
}

/// Times `scope` for as long as it lives: `profile_span!("publish.lanes")`.
#[macro_export]
macro_rules! profile_span {
    ($scope:literal) => {
        let _profile_span = $crate::profile::Span::new($scope);
    };
}

/// Counts something that is not a duration - a request, a dropped frame.
#[inline]
pub fn count(scope: &'static str) {
    if enabled() {
        record(scope, Duration::ZERO);
    }
}

/// Adds a duration measured elsewhere - a wait that began before this call,
/// or work on another thread.
#[inline]
pub fn add(scope: &'static str, took: Duration) {
    if enabled() {
        record(scope, took);
    }
}

/// A number to carry in the table for this second - bytes, frames.
#[inline]
pub fn amount(scope: &'static str, n: u64) {
    if enabled() {
        let mut reg = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = reg.totals.entry(scope).or_default();
        *entry += n;
    }
}

/// One frame reached the screen. The interval since the last is what the
/// eye sees, so it is what the table reports.
#[inline]
pub fn shown() {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let mut reg = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    reg.shown += 1;
    if let Some(previous) = reg.last_shown.replace(now) {
        reg.gaps.push(previous.elapsed());
    }
}

/// One frame was asked for, whether or not it reached the screen.
#[inline]
pub fn requested() {
    if enabled() {
        registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .requested += 1;
    }
}

/// A line of its own for a frame that took too long, with the pieces that
/// made it: `profile::slow_frame("preview", took, &[(name, Duration)])`.
pub fn slow_frame(label: &str, took: Duration, pieces: &[(&str, Duration)]) {
    if !frames_enabled() || took < SLOW_FRAME {
        return;
    }
    let mut detail = String::new();
    for (name, piece) in pieces {
        if piece.is_zero() {
            continue;
        }
        if !detail.is_empty() {
            detail.push_str(" · ");
        }
        let _ = std::fmt::Write::write_fmt(
            &mut detail,
            format_args!("{name} {}", crate::clock::span(*piece)),
        );
    }
    log::debug!("profile: {label} took {} · {detail}", crate::clock::span(took));
}

struct Stat {
    count: u64,
    total: Duration,
    max: Duration,
    /// Nanoseconds, bucketed by power of two: index `k` holds 512 ns to 1 µs
    /// and up, so a percentile is a bucket's upper bound - which is all a
    /// frame budget needs.
    hist: [u32; 33],
}

impl Stat {
    fn new() -> Self {
        Stat {
            count: 0,
            total: Duration::ZERO,
            max: Duration::ZERO,
            hist: [0; 33],
        }
    }
    fn add(&mut self, took: Duration) {
        self.count += 1;
        self.total += took;
        self.max = self.max.max(took);
        let nanos = took.as_nanos().max(1) as u64;
        let bucket = (64 - nanos.leading_zeros()).min(32) as usize;
        self.hist[bucket] += 1;
    }

    /// The upper bound of the bucket the given fraction of runs fall in.
    fn percentile(&self, fraction: f64) -> Duration {
        let want = (self.count as f64 * fraction).ceil() as u64;
        let mut seen = 0u64;
        for (bucket, count) in self.hist.iter().enumerate() {
            seen += u64::from(*count);
            if seen >= want && *count > 0 {
                let upper = 1u64 << bucket.min(32);
                return Duration::from_nanos(upper);
            }
        }
        self.max
    }
}

#[derive(Default)]
struct Registry {
    /// By the thread that ran it, then the scope: a scope that runs on two
    /// threads says so in two rows rather than averaging them into a number
    /// that describes neither.
    scopes: HashMap<(&'static str, &'static str), Stat>,
    /// Counters that are not durations.
    totals: HashMap<&'static str, u64>,
    shown: u64,
    requested: u64,
    last_shown: Option<Instant>,
    gaps: Vec<Duration>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn record(scope: &'static str, took: Duration) {
    let thread = thread_label();
    let mut reg = registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reg.scopes.entry((thread, scope)).or_insert_with(Stat::new).add(took);
}

/// This thread's name, leaked once so a scope can key on it.
fn thread_label() -> &'static str {
    thread_local! {
        static LABEL: &'static str = {
            let name = std::thread::current()
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| "unnamed".to_owned());
            Box::leak(name.into_boxed_str())
        };
    }
    LABEL.with(|label| *label)
}

/// Writes the table for the last `window`, and starts over.
pub fn report(window: Duration) {
    if !enabled() {
        return;
    }
    let mut reg = {
        let mut reg = registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *reg)
    };
    let seconds = window.as_secs_f64().max(0.001);
    let drew = reg.shown as f64 / seconds;
    let rate = if reg.gaps.is_empty() {
        String::new()
    } else {
        let mut gaps = std::mem::take(&mut reg.gaps);
        gaps.sort_unstable();
        let pick = |f: f64| gaps[((gaps.len() as f64 * f) as usize).min(gaps.len() - 1)];
        format!(
            " · shown every {} p50 · {} p95",
            crate::clock::span(pick(0.5)),
            crate::clock::span(pick(0.95)),
        )
    };
    log::info!(
        "profile {:.2} s · {} frames · {drew:.1}/s{rate} · {} asked for",
        seconds,
        reg.shown,
        reg.requested,
    );
    // Slowest first: the first row is the one to look at.
    let mut rows: Vec<_> = reg.scopes.into_iter().collect();
    rows.sort_by_key(|(_, stat)| std::cmp::Reverse(stat.total));
    for ((thread, scope), stat) in rows {
        let average = stat.total.as_secs_f64() * 1000.0 / stat.count.max(1) as f64;
        let share = stat.total.as_secs_f64() / seconds;
        log::info!(
            "  {thread:<10} {scope:<22} {:>5} x {average:>7.2} ms avg · p95 {:>6.2} · max {:>6.2}{}",
            stat.count,
            stat.percentile(0.95).as_secs_f64() * 1000.0,
            stat.max.as_secs_f64() * 1000.0,
            if share >= 0.02 {
                format!(" · {:>5.1}% of the second", share * 100.0)
            } else {
                String::new()
            },
        );
    }
    let mut totals: Vec<_> = std::mem::take(&mut reg.totals).into_iter().collect();
    totals.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (name, n) in totals {
        log::info!("  {name:<10} {n}");
    }
}

/// Writes the table now, for a one-off look.
pub fn flush() {
    if enabled() {
        report(Duration::from_secs(1));
    }
}

/// How many times a counter has been taken, for a caller that wants to
/// decide on the number rather than log it.
pub fn total(name: &str) -> u64 {
    let reg = registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reg.totals.get(name).copied().unwrap_or(0)
}

/// For a caller that wants to read the clock without a span, so two pieces
/// of a frame can be compared after both have run.
pub fn now() -> Instant {
    Instant::now()
}

/// The count of a counter that is only ever added to, shared with the table.
static _RESERVED: AtomicU64 = AtomicU64::new(0);
