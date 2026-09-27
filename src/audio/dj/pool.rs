use crate::audio::dj::weighted_choice::WeightedSelector;

/// Picks an index into a category's item list: weighted by `weight_fn`, with
/// recent-history avoidance and a duplicate penalty from `WeightedSelector`.
///
/// Items are passed into `next` rather than owned here, so a config reload
/// (which swaps the item list) takes effect immediately while the pool's
/// recent-history state carries over unchanged.
pub struct Pool<T> {
    selector: WeightedSelector,
    weight_fn: fn(&T) -> u32,
}

impl<T> Pool<T> {
    pub fn new(
        recent_history_size: usize,
        duplicate_penalty_multiplier: f32,
        weight_fn: fn(&T) -> u32,
    ) -> Self {
        Self {
            selector: WeightedSelector::new(recent_history_size, duplicate_penalty_multiplier),
            weight_fn,
        }
    }

    pub fn next(&mut self, items: &[T]) -> usize {
        self.selector.choose(items, self.weight_fn)
    }
}
