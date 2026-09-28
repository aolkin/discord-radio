use rand::Rng;
use std::collections::VecDeque;
use std::sync::Arc;

/// A weight an item contributes to weighted random selection.
pub trait Weighted {
    fn weight(&self) -> f32;
}

/// A weighted random selector that owns its items: picks an index weighted
/// by `Weighted::weight`, with recent-history avoidance and a duplicate
/// penalty.
pub struct WeightedSelector<T> {
    items: Arc<[T]>,
    recent_history: VecDeque<usize>,
    history_size: usize,
    penalty_multiplier: f32,
}

impl<T: Weighted> WeightedSelector<T> {
    pub fn new(history_size: usize, penalty_multiplier: f32, items: Arc<[T]>) -> Self {
        Self {
            items,
            recent_history: VecDeque::with_capacity(history_size),
            history_size,
            penalty_multiplier,
        }
    }

    /// Swaps the owned item list (e.g. on a config reload) without resetting
    /// the selector, so recent-history state carries over across the swap.
    pub fn set_items(&mut self, items: Arc<[T]>) {
        self.items = items;
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn next(&mut self) -> usize {
        let effective_weights: Vec<f32> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let base_weight = item.weight();
                let penalty = self.get_penalty_for_index(idx);
                base_weight * penalty
            })
            .collect();

        let total: f32 = effective_weights.iter().sum();
        if total == 0.0 {
            return 0;
        }

        let mut rng = rand::rng();
        let roll: f32 = rng.random::<f32>() * total;

        let mut cumulative = 0.0;
        for (idx, weight) in effective_weights.iter().enumerate() {
            cumulative += weight;
            if roll < cumulative {
                self.add_to_history(idx);
                return idx;
            }
        }

        let idx = self.items.len().saturating_sub(1);
        self.add_to_history(idx);
        idx
    }

    fn get_penalty_for_index(&self, idx: usize) -> f32 {
        for (history_idx, &item_idx) in self.recent_history.iter().enumerate() {
            if item_idx == idx {
                let recency_factor = 1.0 - (history_idx as f32 / self.recent_history.len() as f32);
                return self.penalty_multiplier * recency_factor + (1.0 - self.penalty_multiplier);
            }
        }
        1.0
    }

    /// Records `index` into recent-history without selecting, so a
    /// subsequent `next()` treats it as already having just been picked.
    pub fn add_to_history(&mut self, idx: usize) {
        if self.recent_history.len() >= self.history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item(u32);

    impl Weighted for Item {
        fn weight(&self) -> f32 {
            self.0 as f32
        }
    }

    fn items(weights: &[u32]) -> Arc<[Item]> {
        Arc::from(weights.iter().map(|&w| Item(w)).collect::<Vec<_>>())
    }

    #[test]
    fn add_to_history_records_index_without_selecting() {
        let mut selector = WeightedSelector::new(2, 1.0, items(&[1, 1]));
        selector.add_to_history(0);
        assert_eq!(selector.recent_history, VecDeque::from([0]));
    }

    #[test]
    fn set_items_preserves_history() {
        let mut selector = WeightedSelector::new(2, 1.0, items(&[1, 1]));
        selector.add_to_history(0);
        selector.add_to_history(1);
        selector.set_items(items(&[1, 1, 1]));
        assert_eq!(selector.recent_history, VecDeque::from([0, 1]));
    }
}
