use field_traits::{field_impl, field_trait};

#[field_trait]
trait Weighted {
    fn weight(&self) -> u32;
}

struct Track {
    weight: u32,
}

#[field_impl(Weighted)]
impl Weighted for Track {}

fn main() {}
