//! Trajectory sampling and paced rollouts (story-analyze spec §5): the
//! rollout engine for trajectory-guided search.
//!
//! Two-tier macro pacing prevents clock starvation and action chattering:
//! actions are biased toward the causal backward cone of the goal, while
//! advances jump to the next critical due moment.

use std::collections::BTreeSet;
use std::sync::Arc;

use story_content::Content;
use story_core::{State, Time, VerbId};

use crate::causal::{self, CausalGraph};
use crate::query::{GoalCtx, GoalPredicate, RngMode};
use crate::transitions::{PlayerStep, TransitionGenerator, TransitionRecord, WitnessPath};

/// Configuration for trajectory rollouts.
#[derive(Clone, Debug)]
pub struct RolloutConfig {
    /// Maximum transitions along a single rollout.
    pub max_steps: usize,
    /// Clock horizon in minutes.
    pub max_time: Option<Time>,
    /// RNG mode for settlement branching.
    pub rng_mode: RngMode,
    /// Probability of advancing time when situations are actively running.
    pub p_advance: f64,
}

impl Default for RolloutConfig {
    fn default() -> Self {
        RolloutConfig {
            max_steps: 128,
            max_time: None,
            rng_mode: RngMode::Deterministic,
            p_advance: 0.40,
        }
    }
}

/// A paced rollout sampler driving transitions through TransitionGenerator.
pub struct RolloutSampler<'a> {
    pub content: &'a Arc<Content>,
    pub relevant_verbs: BTreeSet<VerbId>,
    pub config: RolloutConfig,
}

impl<'a> RolloutSampler<'a> {
    /// Create a new sampler configured for the given goal.
    pub fn new(
        content: &'a Arc<Content>,
        causal: &'a CausalGraph,
        goal: &GoalPredicate,
        config: RolloutConfig,
    ) -> RolloutSampler<'a> {
        let goal_facts = causal::goal_facts(goal);
        let cone = causal.backward_cone(&goal_facts);
        let relevant_verbs = causal.relevant_verbs(content, &cone);
        RolloutSampler {
            content,
            relevant_verbs,
            config,
        }
    }

    /// Sample one rollout trajectory from `start` using the specified pseudo-RNG seed.
    pub fn sample_path(
        &self,
        generator: &mut TransitionGenerator,
        start: &State,
        goal: &GoalPredicate,
        seed: u64,
    ) -> WitnessPath {
        let mut cur_state = start.clone();
        let mut records: Vec<TransitionRecord> = Vec::new();
        let mut step_count = 0usize;
        let mut pseudo_rng = seed;

        while step_count < self.config.max_steps && cur_state.ended.is_none() {
            if let Some(horizon) = self.config.max_time {
                if cur_state.now >= horizon {
                    break;
                }
            }

            // Check goal on arrival
            generator.install_state(&cur_state);
            let ctx = GoalCtx {
                engine: generator.engine(),
                events: &[],
            };
            if goal.holds(&ctx) {
                break;
            }

            let steps = generator.steps(&cur_state);
            if steps.is_empty() {
                break;
            }

            let mut actions = Vec::new();
            let mut advances = Vec::new();

            for s in &steps {
                match s {
                    PlayerStep::Advance { .. } => advances.push(s.clone()),
                    _ => actions.push(s.clone()),
                }
            }

            // Causal backward cone filtering
            let mut cone_actions: Vec<PlayerStep> = actions
                .iter()
                .filter(|a| match a {
                    PlayerStep::Commit { verb, .. } => self.relevant_verbs.contains(verb),
                    _ => true,
                })
                .cloned()
                .collect();

            let usable_actions = if !cone_actions.is_empty() {
                &mut cone_actions
            } else {
                &mut actions
            };

            // Two-tier macro pacing decision: Act vs Advance
            let has_active_situations = !cur_state.situations.is_empty();
            pseudo_rng = pseudo_rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let rand_val = ((pseudo_rng >> 32) as f64) / (u32::MAX as f64);

            let should_advance = (has_active_situations && rand_val < self.config.p_advance)
                || usable_actions.is_empty();

            let chosen_step = if should_advance && !advances.is_empty() {
                // Pick the earliest due landing
                advances
                    .into_iter()
                    .min_by_key(|a| match a {
                        PlayerStep::Advance { dt } => *dt,
                        _ => i64::MAX,
                    })
                    .unwrap()
            } else if !usable_actions.is_empty() {
                pseudo_rng = pseudo_rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let choice_idx = (pseudo_rng as usize) % usable_actions.len();
                usable_actions.remove(choice_idx)
            } else {
                pseudo_rng = pseudo_rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let choice_idx = (pseudo_rng as usize) % steps.len();
                steps[choice_idx].clone()
            };

            let exec_records = generator.execute(&cur_state, &chosen_step);
            if exec_records.is_empty() {
                break;
            }

            // In deterministic mode, there's 1 record; in branching, pick first outcome
            let rec = exec_records[0].clone();
            cur_state = rec.state_after.clone();
            records.push(rec);
            step_count += 1;
        }

        let actions = records.iter().map(|r| r.actions()).sum();
        let depth = records.len();
        let probability = if matches!(self.config.rng_mode, RngMode::Adversarial) {
            None
        } else {
            Some(records.iter().map(|r| r.probability).product::<f64>())
        };

        WitnessPath {
            start: start.clone(),
            records,
            actions,
            depth,
            probability,
        }
    }
}
