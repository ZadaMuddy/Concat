// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah
//! Where the item walk spends a frame.
//!
//! Every frame, Slint visits each item in the window: works out where it is
//! ([`render_item_children`]), asks the renderer to draw it, then visits its
//! children. On a window with a few hundred items this is the largest single
//! cost of a frame, and nothing above it can say which half of it - the
//! geometry or the drawing - is the expensive one. This says.
//!
//! Only while `CONCAT_PROFILE` is set; off, a visit costs one relaxed load
//! and no clock reads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static FILTER_NANOS: AtomicU64 = AtomicU64::new(0);
static DRAW_NANOS: AtomicU64 = AtomicU64::new(0);
static VISITS: AtomicU64 = AtomicU64::new(0);
static DRAWN: AtomicU64 = AtomicU64::new(0);

/// Whether the walk is being timed.
#[inline]
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CONCAT_PROFILE").is_some())
}

/// Times one visit's two halves. `filter` is the geometry, the bounds and
/// the clip test; `draw` is the dispatch into the renderer.
pub struct Visit {
    start: Option<Instant>,
    filtered: Option<Instant>,
}

impl Visit {
    #[inline]
    pub fn begin() -> Self {
        Visit {
            start: on().then(Instant::now),
            filtered: None,
        }
    }

    /// The geometry is done; the drawing is about to start.
    #[inline]
    pub fn filtered(&mut self) {
        if self.start.is_some() {
            self.filtered = Some(Instant::now());
        }
    }

    /// The visit is over: `drawn` says whether the renderer was asked to
    /// draw it, or the item was only walked past.
    #[inline]
    pub fn end(self, drawn: bool) {
        if let Some(start) = self.start {
            let end = Instant::now();
            let filtered = self.filtered.unwrap_or(end);
            VISITS.fetch_add(1, Ordering::Relaxed);
            if drawn {
                DRAWN.fetch_add(1, Ordering::Relaxed);
            }
            let filter = filtered.saturating_duration_since(start);
            FILTER_NANOS.fetch_add(filter.as_nanos() as u64, Ordering::Relaxed);
            DRAW_NANOS.fetch_add(
                end.saturating_duration_since(filtered).as_nanos() as u64,
                Ordering::Relaxed,
            );
        }
    }
}

/// Takes the totals since the last call: (visits, drawn, geometry, drawing).
pub fn take() -> (u64, u64, Duration, Duration) {
    (
        VISITS.swap(0, Ordering::Relaxed),
        DRAWN.swap(0, Ordering::Relaxed),
        Duration::from_nanos(FILTER_NANOS.swap(0, Ordering::Relaxed)),
        Duration::from_nanos(DRAW_NANOS.swap(0, Ordering::Relaxed)),
    )
}

/// A counter that means nothing on its own; here so the statics above have
/// a companion when they are compiled out.
#[allow(dead_code)]
static RESERVED: AtomicBool = AtomicBool::new(false);
