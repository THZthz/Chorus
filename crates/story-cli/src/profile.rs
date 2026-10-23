//! The `--profile` timing face: a tracing layer that accumulates wall time
//! per span name and prints a sorted table to stderr when the run ends.
//! The instrumented spans live at the phase boundaries — compile, boot, one
//! per analyze section, and the analysis engines' entry points (explore,
//! solve, classify_closure, mdp_solve, pareto_witnesses, pareto_cuts,
//! audit_unreachable, audit_faults). Without `--profile` no subscriber is
//! installed and the spans compile to no-ops.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Registry;

/// One span name's accumulated wall time.
#[derive(Default)]
struct Acc {
    calls: u64,
    total: Duration,
}

#[derive(Default)]
struct State {
    /// Open spans by id: the display name and the last enter instant.
    open: HashMap<tracing::span::Id, (String, Instant)>,
    acc: BTreeMap<String, Acc>,
}

/// The timing layer; the accumulator lives in an `Arc` so the installed
/// subscriber and the global handle share it.
#[derive(Clone)]
pub struct TimingLayer {
    state: Arc<Mutex<State>>,
}

static LAYER: OnceLock<TimingLayer> = OnceLock::new();

/// Install the layer as the global subscriber; a no-op when already
/// installed. Call `report` at the end of the run to print the table.
pub fn install() {
    let layer = LAYER.get_or_init(|| TimingLayer {
        state: Arc::new(Mutex::new(State::default())),
    });
    let _ = tracing::subscriber::set_global_default(Registry::default().with(layer.clone()));
}

impl tracing_subscriber::Layer<Registry> for TimingLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _ctx: Context<'_, Registry>,
    ) {
        let name = attrs.metadata().name().to_string();
        self.state
            .lock()
            .unwrap()
            .open
            .insert(id.clone(), (name, Instant::now()));
    }

    fn on_enter(&self, id: &tracing::span::Id, _ctx: Context<'_, Registry>) {
        // Re-enters restart the clock; the accumulated total is the sum of
        // the entered intervals.
        if let Some(entry) = self.state.lock().unwrap().open.get_mut(id) {
            entry.1 = Instant::now();
        }
    }

    fn on_exit(&self, id: &tracing::span::Id, _ctx: Context<'_, Registry>) {
        let mut state = self.state.lock().unwrap();
        let hit = state
            .open
            .get(id)
            .map(|(name, enter)| (name.clone(), enter.elapsed()));
        if let Some((name, total)) = hit {
            let acc = state.acc.entry(name).or_default();
            acc.calls += 1;
            acc.total += total;
        }
    }

    fn on_close(&self, id: tracing::span::Id, _ctx: Context<'_, Registry>) {
        self.state.lock().unwrap().open.remove(&id);
    }
}

/// Print the accumulated span times to stderr, sorted by total descending.
/// Percentages are inclusive shares of the root `analyze` span's wall time;
/// nested spans count within their parents. A no-op when `--profile` was
/// not given.
pub fn report() {
    let Some(layer) = LAYER.get() else { return };
    let state = layer.state.lock().unwrap();
    let root = state.acc.get("analyze").map_or(Duration::ZERO, |a| a.total);
    if root.is_zero() {
        return;
    }
    let mut rows: Vec<(&String, &Acc)> = state.acc.iter().collect();
    rows.sort_by_key(|(_, acc)| std::cmp::Reverse(acc.total));
    eprintln!("profile — analyze wall {}ms", root.as_millis());
    eprintln!("profile — {:>9}  {:>5}  {:>4}  span", "total", "calls", "%");
    for (name, acc) in rows {
        if name == "analyze" {
            continue;
        }
        eprintln!(
            "profile — {:>8}ms  {:>5}  {:>3.0}%  {name}",
            acc.total.as_millis(),
            acc.calls,
            acc.total.as_secs_f64() / root.as_secs_f64() * 100.0
        );
    }
}
