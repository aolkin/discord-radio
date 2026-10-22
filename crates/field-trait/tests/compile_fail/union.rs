use field_trait::{field_trait, field_trait_impl};

#[field_trait]
trait HasWeight {
    fn weight(&self) -> u32;
}

#[field_trait_impl(HasWeight)]
union Bits {
    weight: u32,
}

fn main() {}
