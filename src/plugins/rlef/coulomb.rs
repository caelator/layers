//! Coulomb repulsion math for the RLEF (Runtime Learned Expression Framework) router.
//!
//! This module implements a "charge" metaphor borrowed from physics: each routing
//! option carries an electrostatic charge. When an option is selected, it gains charge
//! while all other options decay their charge (Coulomb repulsion). Options with higher
//! charge are *less* likely to be selected, enforcing diversity by pushing the algorithm
//! away from recently-chosen routes.

use std::collections::HashMap;

/// Default base weight used in selection before charge adjustment.
pub const DEFAULT_BASE_WEIGHT: f64 = 1.0;

/// Default amount of charge added to a selected option.
pub const DEFAULT_CHARGE_AMOUNT: f64 = 0.5;

/// Default decay factor applied to unselected options after each selection.
pub const DEFAULT_DECAY_FACTOR: f64 = 0.6;

/// Default floor value — no charge may fall below this.
pub const DEFAULT_FLOOR: f64 = 0.05;

/// Maximum allowed charge value (prevent runaway charge accumulation).
pub const CHARGE_MAX: f64 = 1.0;

/// Apply Coulomb repulsion: decay unselected charges and boost the selected option's charge.
///
/// # Algorithm
/// 1. For each option: if it != `selected`, charge *= `decay_factor`
/// 2. `selected.charge = selected.charge * decay_factor + charge_amount`
/// 3. Clamp all charges to [`DEFAULT_FLOOR`, `CHARGE_MAX`]
///
/// Returns the updated charges map.
pub fn apply_coulomb_repulsion(
    charges: &mut HashMap<String, f64>,
    selected: &str,
    charge_amount: f64,
    decay_factor: f64,
    floor: f64,
) {
    // Step 1 & 2: decay unselected, boost selected
    for (name, charge) in charges.iter_mut() {
        if name == selected {
            *charge = *charge * decay_factor + charge_amount;
        } else {
            *charge *= decay_factor;
        }
    }

    // Step 3: clamp all charges to [floor, CHARGE_MAX]
    for charge in charges.values_mut() {
        *charge = charge.clamp(floor, CHARGE_MAX);
    }
}

/// Compute per-candidate weights for weighted-random selection.
///
/// Each candidate's weight = max(floor, `base_weight` - candidate.charge).
/// Options with higher charge receive lower weight, encouraging diversity.
///
/// Returns a vector of (`option_name`, weight) pairs in the same order as `candidates`.
pub fn compute_weights<'a>(
    charges: &'a HashMap<String, f64>,
    candidates: &'a [&'a str],
    base_weight: f64,
    floor: f64,
) -> Vec<(&'a str, f64)> {
    candidates
        .iter()
        .map(|&name| {
            let charge = charges.get(name).copied().unwrap_or(0.0);
            let weight = (base_weight - charge).max(floor);
            (name, weight)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_charge_increases() {
        let mut charges: HashMap<String, f64> =
            vec![("a".into(), 0.2), ("b".into(), 0.3), ("c".into(), 0.1)]
                .into_iter()
                .collect();

        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);

        // a: 0.2 * 0.6 + 0.5 = 0.62
        assert!((charges["a"] - 0.62).abs() < 1e-9);
    }

    #[test]
    fn unselected_charges_decay() {
        let mut charges: HashMap<String, f64> = vec![("a".into(), 0.5), ("b".into(), 0.5)]
            .into_iter()
            .collect();

        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);

        // b: 0.5 * 0.6 = 0.3
        assert!((charges["b"] - 0.3).abs() < 1e-9);
    }

    #[test]
    fn charges_clamp_to_floor() {
        let mut charges: HashMap<String, f64> = vec![("a".into(), 0.01), ("b".into(), 0.01)]
            .into_iter()
            .collect();

        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);

        // b decayed below floor → should be clamped to 0.05
        assert!(charges["b"] >= 0.05);
    }

    #[test]
    fn charges_clamp_to_max() {
        let mut charges: HashMap<String, f64> = vec![("a".into(), 0.95), ("b".into(), 0.95)]
            .into_iter()
            .collect();

        // Repeated selections should push a's charge above max
        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);
        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);
        apply_coulomb_repulsion(&mut charges, "a", 0.5, 0.6, 0.05);

        assert!(charges["a"] <= 1.0);
    }

    #[test]
    fn compute_weights_prefers_lower_charge() {
        let charges: HashMap<String, f64> = vec![("a".into(), 0.5), ("b".into(), 0.2)]
            .into_iter()
            .collect();

        let weights = compute_weights(&charges, &["a", "b"], 1.0, 0.05);

        // a: 1.0 - 0.5 = 0.5, b: 1.0 - 0.2 = 0.8
        assert!((weights[0].1 - 0.5).abs() < 1e-9);
        assert!((weights[1].1 - 0.8).abs() < 1e-9);
    }

    #[test]
    fn compute_weights_respects_floor() {
        let charges: HashMap<String, f64> = vec![("a".into(), 0.99)].into_iter().collect();

        let weights = compute_weights(&charges, &["a"], 1.0, 0.05);

        // 1.0 - 0.99 = 0.01 < floor(0.05) → should be clamped to floor
        assert!((weights[0].1 - 0.05).abs() < 1e-9);
    }

    #[test]
    fn compute_weights_unknown_option_gets_base() {
        let charges: HashMap<String, f64> = HashMap::new();
        let weights = compute_weights(&charges, &["unknown"], 1.0, 0.05);
        // unknown has no charge → weight = base_weight - 0 = 1.0
        assert!((weights[0].1 - 1.0).abs() < 1e-9);
    }
}
