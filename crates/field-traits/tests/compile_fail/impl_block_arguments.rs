use field_traits::{field_trait, field_trait_impl};

#[field_trait]
trait Weighted {
    fn weight(&self) -> u32;
}

struct Track {
    weight: u32,
}

#[field_trait_impl(Weighted)]
impl Weighted for Track {}

fn main() {}
