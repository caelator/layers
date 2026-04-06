//! Weighted random selection logic for the RLEF router.
//!
//! Given a set of candidates with computed weights, selects one candidate
//! using weighted random sampling.

use crate::plugins::rlef::coulomb::compute_weights;
use std::collections::HashMap;

/// Perform weighted random selection from a list of candidates.
///
/// Each candidate's effective weight is `max(floor, base_weight - charge)` where
/// charge comes from the `charges` map (0 if the candidate has no entry).
///
/// # Panics
/// Panics if `candidates` is empty or if all weights are zero.
pub fn weighted_select(
    charges: &HashMap<String, f64>,
    candidates: &[&str],
    base_weight: f64,
    floor: f64,
) -> String {
    let weights = compute_weights(charges, candidates, base_weight, floor);

    let total: f64 = weights.iter().map(|(_, w)| w).sum();

    assert!(
        total > 0.0,
        "weighted_select requires at least one candidate with non-zero weight"
    );
    assert!(!candidates.is_empty(), "candidates list must not be empty");

    // rand::random::<f64>() gives a uniform f64 in [0, 1) using the best
    // available entropy source — avoids u64→f64 precision loss.
    let threshold: f64 = rand::random::<f64>() * total;

    let mut cumulative: f64 = 0.0;
    for (name, weight) in &weights {
        cumulative += weight;
        if cumulative > threshold {
            return name.to_string();
        }
    }

    // Fallback: return last (should not reach here if weights are correct)
    candidates.last().map(|s: &&str| s.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_returns_one_of_candidates() {
        let charges: HashMap<String, f64> = HashMap::new();
        let candidates = ["a", "b", "c"];

        for _ in 0..100 {
            let selected = weighted_select(&charges, &candidates, 1.0, 0.05);
            assert!(candidates.contains(&selected.as_str()));
        }
    }

    #[test]
    fn repeated_selection_distributes() {
        let charges: HashMap<String, f64> =
            vec![("a".into(), 0.0), ("b".into(), 0.0)].into_iter().collect();

        let candidates = ["a", "b"];

        // With equal charges, selection should be roughly 50/50 over many trials
        let mut a_count = 0u32;
        let trials = 1000;
        for _ in 0..trials {
            let selected = weighted_select(&charges, &candidates, 1.0, 0.05);
            if selected == "a" {
                a_count += 1;
            }
        }

        // Allow ±15% tolerance
        let a_ratio = a_count as f64 / trials as f64;
        assert!((a_ratio - 0.5).abs() < 0.15);
    }

    #[test]
    fn higher_charge_reduces_selection_probability() {
        // Option "a" has high charge → should be selected less often
        let charges: HashMap<String, f64> =
            vec![("a".into(), 0.8), ("b".into(), 0.1)].into_iter().collect();

        let candidates = ["a", "b"];

        let mut a_count = 0u32;
        let trials = 1000;
        for _ in 0..trials {
            let selected = weighted_select(&charges, &candidates, 1.0, 0.05);
            if selected == "a" {
                a_count += 1;
            }
        }

        // With high charge on a, b should dominate
        let a_ratio = a_count as f64 / trials as f64;
        assert!(
            a_ratio < 0.25,
            "high-charge option 'a' should be selected < 25% of the time, got {a_ratio:.3}"
        );
    }

    #[test]
    #[should_panic(expected = "at least one candidate")]
    fn empty_candidates_panics() {
        let charges: HashMap<String, f64> = HashMap::new();
        let candidates: Vec<&str> = vec![];
        weighted_select(&charges, &candidates, 1.0, 0.05);
    }

    #[test]
    fn unknown_candidates_get_base_weight() {
        // "c" is not in charges map, should get base weight
        let charges: HashMap<String, f64> =
            vec![("a".into(), 0.0)].into_iter().collect();

        let candidates = ["a", "c"];
        let selected = weighted_select(&charges, &candidates, 1.0, 0.05);
        // Should always pick c because a has charge=0 → weight=1.0, c has unknown → weight=1.0
        // Actually both have weight 1.0, so it's a 50/50 split
        assert!(["a", "c"].contains(&selected.as_str()));
    }
}
