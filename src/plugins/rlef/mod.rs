//! RLEF — Runtime Learned Expression Framework.
//!
//! A diversity-enforcing selection algorithm that balances routing options
//! (e.g., `memory_only`, `graph_only`, `both`, `neither`) by tracking a "charge"
//! value per option. When an option is selected, its charge increases while all
//! others decay (Coulomb repulsion). Options with higher charge are *less* likely
//! to be selected on the next round, preventing any single route from dominating.
//!
//! A floor of 5% ensures no option is ever fully excluded.
//!
//! # Example
//! ```
//! use layers::plugins::rlef::RlefRouterPlugin;
//!
//! let mut router = RlefRouterPlugin::new();
//! let candidates = ["memory_only", "graph_only", "both", "neither"];
//!
//! // First selection is uniform random (no charges yet)
//! let chosen = router.select(&candidates);
//! router.record_selection(&chosen);
//!
//! // Second selection: the charged option is now less likely
//! let chosen2 = router.select(&candidates);
//! ```
//!
//! Charges are serializable so the router state can be persisted to disk
//! between process invocations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[allow(dead_code)]
pub mod coulomb;
#[allow(dead_code)]
pub mod selection;

/// Configuration parameters for the RLEF router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlefConfig {
    /// Base weight before charge adjustment.
    pub base_weight: f64,
    /// Charge added to the selected option on each selection.
    pub charge_amount: f64,
    /// Decay factor applied to unselected options after each selection.
    pub decay_factor: f64,
    /// Minimum charge value (floor).
    pub floor: f64,
}

impl Default for RlefConfig {
    fn default() -> Self {
        Self {
            base_weight: coulomb::DEFAULT_BASE_WEIGHT,
            charge_amount: coulomb::DEFAULT_CHARGE_AMOUNT,
            decay_factor: coulomb::DEFAULT_DECAY_FACTOR,
            floor: coulomb::DEFAULT_FLOOR,
        }
    }
}

/// RLEF — Runtime Learned Expression Framework router.
///
/// Tracks per-option charges and performs weighted-random selection that
/// discourages recently-chosen options, enforcing routing diversity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RlefRouterPlugin {
    /// Per-option charge values. Higher charge → less likely to be selected.
    charges: HashMap<String, f64>,
    /// Base weight used in weight computation before charge subtraction.
    base_weight: f64,
    /// Amount of charge added to the selected option.
    charge_amount: f64,
    /// Decay factor applied to unselected options after each selection.
    decay_factor: f64,
    /// Minimum allowed charge (floor).
    floor: f64,
}

impl Default for RlefRouterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl RlefRouterPlugin {
    /// Construct a new RLEF router with default parameters.
    ///
    /// Default values:
    /// - `base_weight`: 1.0
    /// - `charge_amount`: 0.5
    /// - `decay_factor`: 0.6
    /// - `floor`: 0.05
    #[must_use]
    pub fn new() -> Self {
        Self {
            charges: HashMap::new(),
            base_weight: coulomb::DEFAULT_BASE_WEIGHT,
            charge_amount: coulomb::DEFAULT_CHARGE_AMOUNT,
            decay_factor: coulomb::DEFAULT_DECAY_FACTOR,
            floor: coulomb::DEFAULT_FLOOR,
        }
    }

    /// Construct a new RLEF router with custom parameters.
    ///
    /// # Panics
    /// Panics if any parameter is not finite, negative, or if `floor >= base_weight`.
    #[must_use]
    pub fn with_config(config: RlefConfig) -> Self {
        assert!(config.base_weight.is_finite());
        assert!(config.charge_amount.is_finite());
        assert!(config.decay_factor.is_finite());
        assert!(config.floor.is_finite());
        assert!(config.decay_factor > 0.0 && config.decay_factor < 1.0);
        assert!(config.floor >= 0.0);
        assert!(
            config.floor < config.base_weight,
            "floor ({}) must be less than base_weight ({})",
            config.floor,
            config.base_weight
        );

        Self {
            charges: HashMap::new(),
            base_weight: config.base_weight,
            charge_amount: config.charge_amount,
            decay_factor: config.decay_factor,
            floor: config.floor,
        }
    }

    /// Initialize or reset charge for a specific option to zero.
    ///
    /// If the option already has a charge, this resets it to zero.
    pub fn init_charge(&mut self, option: &str) {
        self.charges.insert(option.to_string(), 0.0);
    }

    /// Initialize charges for a set of options.
    ///
    /// Any option not in `options` is removed from the charge map.
    pub fn init_charges(&mut self, options: &[&str]) {
        let new_options: HashMap<String, f64> = options
            .iter()
            .map(|&s| (s.to_string(), 0.0))
            .collect();
        self.charges = new_options;
    }

    /// Record that a particular routing option was selected.
    ///
    /// Applies Coulomb repulsion:
    /// - Selected option gains charge: `charge = charge * decay_factor + charge_amount`
    /// - Unselected options decay: `charge *= decay_factor`
    /// - All charges are clamped to `[floor, 1.0]`
    ///
    /// If the option is not yet in the charge map, it is initialized with zero charge
    /// before the update is applied.
    pub fn record_selection(&mut self, option: &str) {
        // Ensure all candidates are in the map with at least floor charge
        if !self.charges.contains_key(option) {
            self.charges.insert(option.to_string(), 0.0);
        }

        coulomb::apply_coulomb_repulsion(
            &mut self.charges,
            option,
            self.charge_amount,
            self.decay_factor,
            self.floor,
        );
    }

    /// Select a routing option from the given candidates using weighted random selection.
    ///
    /// Each candidate's weight = `max(floor, base_weight - candidate.charge)`.
    /// Options with higher charge receive lower weight, promoting diversity.
    ///
    /// Returns the name of the selected option.
    ///
    /// # Panics
    /// Panics if `candidates` is empty.
    #[must_use]
    pub fn select(&self, candidates: &[&str]) -> String {
        selection::weighted_select(
            &self.charges,
            candidates,
            self.base_weight,
            self.floor,
        )
    }

    /// Get the current charge value for an option.
    ///
    /// Returns `None` if the option has never been recorded.
    #[must_use]
    pub fn charge(&self, option: &str) -> Option<f64> {
        self.charges.get(option).copied()
    }

    /// Get a snapshot of all charges.
    #[must_use]
    pub fn all_charges(&self) -> &HashMap<String, f64> {
        &self.charges
    }

    /// Reset all charges to zero and clear the charge map.
    pub fn reset(&mut self) {
        self.charges.clear();
    }

    /// Reset only the charges for the given options, leaving others untouched.
    pub fn reset_charges(&mut self, options: &[&str]) {
        for &opt in options {
            self.charges.insert(opt.to_string(), 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_empty_charges() {
        let router = RlefRouterPlugin::new();
        assert!(router.all_charges().is_empty());
    }

    #[test]
    fn record_selection_updates_charges() {
        let mut router = RlefRouterPlugin::new();
        router.init_charges(&["a", "b"]);

        router.record_selection("a");

        // a should have gained charge, b should have decayed
        assert!(router.charge("a").is_some());
        assert!(router.charge("b").is_some());
        assert!(router.charge("a") >= router.charge("b"));
    }

    #[test]
    fn repeated_selection_same_option_makes_it_less_likely() {
        let mut router = RlefRouterPlugin::new();
        router.init_charges(&["a", "b"]);

        // Select "a" many times
        for _ in 0..20 {
            router.record_selection("a");
        }

        // Now "a" should have high charge and be selected less often
        let mut a_count = 0u32;
        let trials = 500;
        let candidates = ["a", "b"];

        for _ in 0..trials {
            let selected = router.select(&candidates);
            if selected == "a" {
                a_count += 1;
            }
        }

        let a_ratio = a_count as f64 / trials as f64;
        // a should be selected less than 30% of the time (much lower than 50%)
        assert!(
            a_ratio < 0.30,
            "after 20 selections, 'a' should be demoted to < 30% selection rate, got {a_ratio:.3}"
        );
    }

    #[test]
    fn floor_never_drops_below_floor() {
        let mut router = RlefRouterPlugin::new();
        router.init_charges(&["a", "b", "c"]);

        // Repeatedly select one option to drive others to floor
        for _ in 0..50 {
            router.record_selection("a");
        }

        for opt in ["b", "c"] {
            let c = router.charge(opt).expect("option should exist");
            assert!(
                c >= router.floor - 1e-9,
                "charge for {opt} should never drop below floor, got {c}, floor = {}",
                router.floor
            );
        }
    }

    #[test]
    fn reset_clears_all_charges() {
        let mut router = RlefRouterPlugin::new();
        router.init_charges(&["a", "b"]);
        router.record_selection("a");
        router.record_selection("a");

        router.reset();

        assert!(router.all_charges().is_empty());
    }

    #[test]
    fn select_returns_one_of_candidates() {
        let router = RlefRouterPlugin::new();
        let candidates = ["memory_only", "graph_only", "both", "neither"];

        for _ in 0..50 {
            let selected = router.select(&candidates);
            assert!(
                candidates.contains(&selected.as_str()),
                "select must return one of the candidates, got '{selected}'"
            );
        }
    }

    #[test]
    fn charges_are_serializable() {
        let mut router = RlefRouterPlugin::new();
        router.init_charges(&["a", "b"]);
        router.record_selection("a");
        router.record_selection("a");
        router.record_selection("b");

        let json = serde_json::to_string(&router).expect("must serialize");
        let restored: RlefRouterPlugin =
            serde_json::from_str(&json).expect("must deserialize");

        // Charges should round-trip (f64 precision may differ slightly)
        assert_eq!(
            router.all_charges().len(),
            restored.all_charges().len()
        );
        for (k, v) in router.all_charges() {
            let rv = restored.all_charges().get(k);
            assert!(
                rv.is_some(),
                "restored map missing key '{k}'"
            );
            assert!(
                (v - rv.unwrap()).abs() < 1e-6,
                "charge mismatch for '{k}': {v} vs {}",
                rv.unwrap()
            );
        }
    }

    #[test]
    fn unknown_option_gets_initialized_on_record() {
        let mut router = RlefRouterPlugin::new();
        // Don't call init_charges — just record_selection directly

        router.record_selection("brand_new_option");

        assert!(router.charge("brand_new_option").is_some());
    }

    #[test]
    fn with_config_respects_custom_values() {
        let config = RlefConfig {
            base_weight: 2.0,
            charge_amount: 0.3,
            decay_factor: 0.7,
            floor: 0.1,
        };
        let router = RlefRouterPlugin::with_config(config);
        assert_eq!(router.base_weight, 2.0);
        assert_eq!(router.charge_amount, 0.3);
        assert_eq!(router.decay_factor, 0.7);
        assert_eq!(router.floor, 0.1);
    }
}
