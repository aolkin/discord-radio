use field_trait::{field_trait, field_trait_impl};

#[field_trait]
trait HasWeight {
    fn weight(&self) -> u32;
}

struct Widget {
    weight: u32,
}

#[field_trait_impl(HasWeight)]
impl HasWeight for Widget {}

struct Gadget {
    weight: u32,
}

#[field_trait_impl(heft = weight)]
impl HasWeight for Gadget {}

fn main() {}
