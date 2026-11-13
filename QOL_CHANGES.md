# Changes on this fork (`QOL-Changes`)

This file is **local to this fork**. `docs/` in this repository is the
upstream project's user documentation and is not the place for it; this is a
record of what was changed here and why, so any of it can be rebuilt,
explained, or undone later.

Each entry says what changed, why, where it lives, and how to check it.

---

## 1. Splitting: one verb, on the selection only

**What.** The little razor button that rode the playhead is gone; the playhead
is a clean line. `Ctrl+B` / `B` now cut only the clips that are *selected* and
that the playhead actually runs through. With nothing selected, nothing is cut
and a notice says so.

**Why.** Splitting used to reach into every unlocked track under the playhead,
so a caption or a music bed got cut along with the picture. Clicking empty lane
space also moved the playhead, which made it impossible to clear a selection
without losing your place.

**Where.** `ui/timeline/lanes.slint`, `ui/workspace/timeline-pane.slint`,
`src/studio/menus.rs` (`split_at`), `src/studio/keyboard.rs`, `src/lib.rs`.

---

## 2. The playhead is a grip, and it snaps

**What.** The green playhead line can be dragged from anywhere down its length,
not only from the ruler. While dragging, it pulls to a clip's start or end
within 8 px (the same reach a clip drag has), under the existing Snap switch —
the magnet button in the tray, `N`, or the menu row. A **click** still lands
exactly where it is put; only a drag is helped.

**Why.** The ruler was the only scrub surface and a 1 px line is not a target.

**Where.** The grip and the snap call sites are in `ui/timeline/lanes.slint`;
the seek variant is `scrubbed-drag`, threaded through
`ui/workspace/timeline-pane.slint`, `ui/workspace/seat.slint`,
`ui/editor.slint` and `ui/phone/editor.slint` to `on_scrubbed_drag` in
`src/lib.rs`; the snapping is `studio::scrub_snapped` (`src/studio/monitor.rs`)
over `Studio::snapped_instant` (`src/studio/edit.rs`).

**Note.** `snapped_instant` deliberately excludes the playhead from its own
target list. Reusing the clip-side `snapped()` would make the playhead snap to
itself and never move.

---

## 3. Scrolling: thirty seconds is a *resting* rule

**What.** The view may be left at most 30 s past the end of the cut
(`SCROLL_PAST_END`), and may not be zoomed out further than the cut plus that
room filling the pane. While a gesture is running, **no bound is applied at
all** — the drag goes where it is carried, the view follows it, and the rule
returns on release.

**Why.** The 30 s rule is about where the view may sit when nobody is touching
it. Applied during a drag it became a wall at exactly the moment the person was
trying to extend past it.

**Where.** `src/panes/timeline.rs` (`clamp_zoom`, `clamp_scroll`,
`max_scroll`, `SCROLL_PAST_END`), `src/studio/edit.rs` (`gesturing`, and the
re-clamp in `after_change`), `src/studio/publish.rs` (`duration-seconds` is
published from the echo during a gesture), `ui/workspace/timeline-pane.slint`
(the `max-scroll` the bar and the lanes share).

**How to check.** Zoom in until the cut is wider than the view, scroll to the
right end, remove a track that has clips on it, then drag a clip right: it must
still scroll. (It used to stop for good — the clamp had been skipped on edits,
so the view stayed parked past a now-shorter cut and the edge scroll's own test
read false forever.)

---

## 4. The edge scroll is a speed, not a step

**What.** Holding a drag in the 40 px edge zone carries the view at 120–1600
px/s depending on how far past the zone the pointer is, applied as a
*fractional* number of pixels per 16 ms tick, capped at 2 % of the visible span
per tick.

**Why.** It used to add whole pixels a tick, gated on the pointer having moved
since the last tick. That made a drag feel pumped by hand rather than carried:
the view advanced only when the person twitched, and then in jumps.

**Where.** `edge-speed()`, `edge-depth` and the `edge-scroll` timer in
`ui/timeline/lanes.slint`.

---

## 5. What was tried and does not work (so it is not tried again)

- **Clamping the dragged clip to the view's right edge.** Twice. It freezes
  one direction the moment the clip reaches the edge, and again when the view
  has scrolled past the position the clamp measures from (the clamp used
  press-time positions, which go stale).
- **Bounding the scroll by the committed cut during a drag.** The clip cannot
  extend the timeline past a line drawn before the drag began.
- **Gating the edge scroll on pointer movement.** The view only advances when
  the hand twitches — chunky, and progress depends on the mouse.
- **Computing `edge-depth` or the view span as a length ÷ length.** The ratio
  passes values several times its ceiling, and the ramp built on it runs an
  order of magnitude fast. Divide by `1px` first so the maths is in floats.

---

## 6. Temporary: debug probes in the drag path

`src/studio/gestures.rs` logs, at `debug` level only:

- `press <id>: MOVE ...` / `press <id>: TRIM ...` — which gesture a press began;
- `drag: seconds … shift … view_right … scroll … width … spp` — one line per
  drag event.

Run with `CONCAT_LOG=debug` and the lines land in
`%LOCALAPPDATA%\app.concat.editor\logs\concat-*.log`. They are silent at the
normal level. **Remove them before any pull request** — they are diagnosis, not
product.

---

## 7. Working notes for this machine

- Build with `cd src && cargo dev` (an alias for `cargo run --bin concat
  --profile quick`). Never from the repository root.
- `FFMPEG_DIR` and `LIBCLANG_PATH` are set in the user environment; `vendor/`
  holds the FFmpeg development build and is gitignored.
- Windows Defender real-time scanning turns a `.slint` rebuild from ~3 minutes
  into ~15. Exclusions for `src/target` and `%USERPROFILE%\.cargo` keep
  iteration fast.
- A `.slint` edit recompiles the whole UI: `slint-build` generates one ~82 MB,
  ~964k-line `app.rs` for all 79 files. It is structural, not a setting.
- Closing the window with `Stop-Process` trips the app's safe-start flag and
  drops the next launch into software rendering. Close it from the window.
