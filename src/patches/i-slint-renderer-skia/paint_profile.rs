// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah
//! What the Skia item renderer spends a frame's draw time on.
//!
//! `wgpupaint` in the surface says how long a frame's draw took; this says
//! which kind of thing it was drawing. Every call into the item renderer is
//! weighed, the nanoseconds are added to its kind, and once every twentieth
//! frame the kinds are printed beside the frame's own total:
//!
//! ```text
//! items: image 8.90 ms/23 · text 2.10 ms/85 · rect 0.62 ms/412 · path 0.01 ms/2
//! ```
//!
//! Only while `CONCAT_PROFILE` is set. Off, a scope is one relaxed load and
//! the timestamps are never taken.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const IMAGE: usize = 0;
pub const RECT: usize = 1;
pub const BORDER: usize = 2;
pub const TEXT: usize = 3;
pub const TEXT_INPUT: usize = 4;
pub const PATH: usize = 5;
pub const SHADOW: usize = 6;
pub const PIXMAP: usize = 7;
pub const STRING: usize = 8;
pub const GLYPH: usize = 9;
pub const WINDOW_BACKGROUND: usize = 10;
/// Not a kind of item: the time between one item's draw and the next, which
/// is the item tree being walked - geometry, clipping, visibility - and is
/// where a frame's time goes when the draw calls themselves are cheap.
pub const GAP: usize = 11;

pub const KINDS: usize = 12;
pub const NAMES: [&str; KINDS] = [
    "image",
    "rect",
    "border",
    "text",
    "text-input",
    "path",
    "shadow",
    "pixmap",
    "string",
    "glyph",
    "background",
    "gap",
];

const ZERO: AtomicU64 = AtomicU64::new(0);
static NANOS: [AtomicU64; KINDS] = [ZERO; KINDS];
static CALLS: [AtomicU64; KINDS] = [ZERO; KINDS];

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CONCAT_PROFILE").is_some())
}

/// Times one draw call of `kind`, for as long as it lives.
#[must_use]
pub struct Scope {
    kind: usize,
    at: Option<Instant>,
}

impl Scope {
    #[inline]
    pub fn new(kind: usize) -> Self {
        if !on() {
            return Scope { kind, at: None };
        }
        let now = Instant::now();
        LAST.with(|last| {
            if let Some(previous) = last.get() {
                let gap = now.duration_since(previous).as_nanos() as u64;
                NANOS[GAP].fetch_add(gap, Ordering::Relaxed);
                CALLS[GAP].fetch_add(1, Ordering::Relaxed);
            }
        });
        Scope { kind, at: Some(now) }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(at) = self.at {
            LAST.with(|last| last.set(Some(Instant::now())));
            let took = at.elapsed().as_nanos() as u64;
            NANOS[self.kind].fetch_add(took, Ordering::Relaxed);
            CALLS[self.kind].fetch_add(1, Ordering::Relaxed);
        }
    }
}

thread_local! {
    /// When the last draw call ended on this thread, so the next one can say
    /// how long the walk to it took. Cleared at the start of every frame, so
    /// the first call's gap is the frame's own setup.
    static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Starts a frame: the first draw call's gap is then the frame's setup.
pub fn frame_start() {
    if on() {
        LAST.with(|last| last.set(None));
    }
}

/// Prints what has been drawn since the last call, and starts over.
pub fn dump(frame_draw: Duration) {
    if !on() {
        return;
    }
    let mut detail = String::new();
    for kind in 0..KINDS {
        let calls = CALLS[kind].swap(0, Ordering::Relaxed);
        let nanos = NANOS[kind].swap(0, Ordering::Relaxed);
        if calls == 0 {
            continue;
        }
        let ms = nanos as f64 / 1_000_000.0;
        if ms < 0.05 {
            continue;
        }
        if !detail.is_empty() {
            detail.push_str(" · ");
        }
        detail.push_str(&format!("{} {ms:.2} ms/{calls}", NAMES[kind]));
    }
    eprintln!(
        "items: {}  (draw {:.2} ms)",
        if detail.is_empty() { "nothing over 0.05 ms" } else { &detail },
        frame_draw.as_secs_f64() * 1000.0,
    );
}

/// For a caller that wants a one-off read without a frame boundary.
pub fn enabled() -> bool {
    static ANY: AtomicBool = AtomicBool::new(true);
    let _ = ANY.load(Ordering::Relaxed);
    on()
}
