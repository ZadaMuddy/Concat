// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah
//! How fast can this machine present a window at all?
//!
//! A one-millisecond timer changes a property, which the window has to
//! paint; the timer's own gaps are therefore the loop's turnaround - the
//! same number the editor's `loop.tick_gap` reports, with no editor in the
//! way.
//!
//! ```text
//! cargo run --profile quick -p concat --example present_probe
//! ```
//!
//! A trivial window that cannot beat twenty a second is the compositor's
//! doing, and no optimisation of the editor can go faster than it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::slint! {
    export component Probe inherits Window {
        in property <int> ticks: 0;
        // The size of the editor's window, and a filled body, so this is a
        // fair question: a bare 16x16 window presents at hundreds a second
        // and says nothing about a 2560x1440 one.
        preferred-width: 1400px;
        preferred-height: 900px;
        background: #202020;
        Rectangle {
            x: 0; y: 0;
            width: 100%; height: 100%;
            background: #303840;
        }
        Text {
            text: root.ticks;
            color: #e8e8e8;
            font-size: 96px;
            horizontal-alignment: center;
            vertical-alignment: center;
        }
    }
}

fn main() {
    let app = Probe::new().expect("a window");
    let window = app.as_weak();
    // Built inside the loop: the callback is `Send` and these are not. A
    // timer started before the loop runs is never registered with it.
    slint::invoke_from_event_loop(move || {
        let ticks = Rc::new(Cell::new(0u64));
        let gaps: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
        let last = Rc::new(Cell::new(Instant::now()));
        let timer = Rc::new(slint::Timer::default());
        let start = timer.clone();
        start.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(1),
            move || {
                let now = Instant::now();
                {
                    let mut list = gaps.borrow_mut();
                    list.push(now.duration_since(last.get()).as_secs_f64() * 1000.0);
                    last.set(now);
                    if list.len() >= 100 {
                        let mut sorted = list.clone();
                        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        let median = sorted[sorted.len() / 2];
                        let p95 = sorted[sorted.len() * 95 / 100];
                        let worst = sorted[sorted.len() - 1];
                        println!(
                            "probe: median {median:.1} ms · p95 {p95:.1} · worst {worst:.1} · {:.1} a second",
                            1000.0 / median
                        );
                        list.clear();
                    }
                }
                let n = ticks.get() + 1;
                ticks.set(n);
                if let Some(app) = window.upgrade() {
                    app.set_ticks((n % 10_000) as i32);
                }
            },
        );
        std::mem::forget(timer);
    })
    .ok();
    app.run().expect("an event loop");
}
