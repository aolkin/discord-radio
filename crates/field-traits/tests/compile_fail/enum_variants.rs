use field_traits::{field_trait, field_trait_impl};

#[field_trait]
trait HasWeight {
    fn weight(&self) -> u32;
}

#[field_trait_impl(HasWeight)]
enum Item {
    Unit,
    Pair(u32, u32),
    Named { weight: u32 },
}

fn main() {}
