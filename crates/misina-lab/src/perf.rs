//! Where a rebuild's time goes.
//!
//! What it is for: a param change re-runs one layer and everything downstream
//! of it, and which layer that is decides whether a slider drags smoothly or
//! stalls. A leaf knob re-clusters six figures of blades; a planting knob only
//! moves things about. The breakdown is how you tell those apart.
//!
//! [`plugin`], which [`viewer::app`](crate::viewer::app) adds when
//! `{PACKAGE}_PERF` is set, brackets [`Build`] and the `Update` / `PostUpdate`
//! schedules with marker systems and logs a breakdown on any frame that
//! actually rebuilt something:
//!
//! ```text
//! VINERYLAB_PERF=1 cargo run
//! ```
//!
//! The per-layer breakdown is the generator's to place: it adds a
//! [`mark`] between each pair of its build systems and writes
//! [`Perf::changed`] from its own params resources, from a plugin of its own
//! that its `main` adds under the same variable.
//!
//! The marks are wall-clock deltas between systems, so each one is "whatever
//! ran since the previous mark" rather than a true per-system total. That is
//! precise enough because [`plugin`] pins `PreUpdate` to a single-threaded
//! executor, which fixes the order the marks sit in.

use std::time::Instant;

use bevy::prelude::*;

use crate::Build;

/// Elapsed-time marks for the frame in progress.
#[derive(Resource)]
pub struct Perf {
    last: Instant,
    pub marks: Vec<(&'static str, f64)>,
    pub frames: u64,
    /// Params resources marked changed this frame — the gate every build
    /// system's `run_if` reads. The generator's perf plugin writes it.
    pub changed: String,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            marks: Vec::new(),
            frames: 0,
            changed: String::new(),
        }
    }
}

impl Perf {
    fn mark(&mut self, label: &'static str) {
        let now = Instant::now();
        self.marks
            .push((label, (now - self.last).as_secs_f64() * 1e3));
        self.last = now;
    }

    fn reset(&mut self) {
        self.marks.clear();
        self.last = Instant::now();
        self.frames += 1;
    }

    /// Total of the marks whose label starts with `prefix`.
    pub fn total(&self, prefix: &str) -> f64 {
        self.marks
            .iter()
            .filter(|(l, _)| l.starts_with(prefix))
            .map(|(_, ms)| ms)
            .sum()
    }

    pub fn line(&self) -> String {
        self.marks
            .iter()
            .filter(|(_, ms)| *ms > 0.05)
            .map(|(l, ms)| format!("{l} {ms:.1}ms"))
            .collect::<Vec<_>>()
            .join("  ")
    }
}

/// A system that records the time since the previous mark under `label`.
/// Labels starting with `author:` count as authoring in [`report`].
pub fn mark(label: &'static str) -> impl Fn(ResMut<Perf>) + Clone + Send + Sync + 'static {
    move |mut perf: ResMut<Perf>| perf.mark(label)
}

fn reset(mut perf: ResMut<Perf>) {
    perf.reset();
}

/// Brackets [`Build`], plus the `Update` and `PostUpdate` schedules. The marks
/// are deltas, so each one is the cost of whatever ran since the previous
/// mark.
pub fn plugin(app: &mut App) {
    // The marks are wall-clock points inside the schedule, so they only
    // attribute cost to one system if nothing else runs alongside them.
    app.edit_schedule(PreUpdate, |s| {
        s.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.init_resource::<Perf>()
        .add_systems(PreUpdate, reset.before(Build))
        // Schedule-level brackets: `RunFixedMainLoop` runs between `PreUpdate`
        // and `Update`, so the next mark covers all of `Update`.
        .add_systems(bevy::app::RunFixedMainLoop, mark("author:tail"))
        .add_systems(PostUpdate, mark("update"))
        .add_systems(Last, (mark("postupdate"), report).chain());
}

/// Logs the breakdown on frames that actually re-authored something.
pub fn report(perf: Res<Perf>) {
    let authored = perf.total("author:");
    if authored < 0.5 {
        return;
    }
    let total: f64 = perf.marks.iter().map(|(_, ms)| ms).sum();
    info!(
        "frame {}: {total:.1}ms total | author {authored:.1}ms | changed [{}] | {}",
        perf.frames,
        perf.changed,
        perf.line()
    );
}
