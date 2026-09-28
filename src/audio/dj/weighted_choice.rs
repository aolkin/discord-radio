use rand::Rng;
use std::collections::VecDeque;

/// A weight an item contributes to weighted random selection.
pub trait Weighted {
    fn weight(&self) -> f32;
}

/// A weighted random selector with history-based penalty to avoid repetition
pub struct WeightedSelector {
    recent_history: VecDeque<usize>,
    history_size: usize,
    penalty_multiplier: f32,
}

impl WeightedSelector {
    pub fn new(history_size: usize, penalty_multiplier: f32) -> Self {
        Self {
            recent_history: VecDeque::with_capacity(history_size),
            history_size,
            penalty_multiplier,
        }
    }

    pub fn choose<T: Weighted>(&mut self, items: &[T]) -> usize {
        let effective_weights: Vec<f32> = items
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

        let idx = items.len().saturating_sub(1);
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
    /// subsequent `choose` treats it as already having just been picked.
    pub fn add_to_history(&mut self, idx: usize) {
        if self.recent_history.len() >= self.history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(idx);
    }
}
