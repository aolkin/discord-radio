use field_traits::{field_fill, field_trait};

#[field_trait]
trait Weighted {
    fn weight(&self) -> u32;

    fn doubled_weight(&self) -> u32 {
        self.weight() * 2
    }
}

struct Track {
    weight: u32,
}

#[field_fill(weight, doubled_weight)]
impl Weighted for Track {}

fn main() {}
