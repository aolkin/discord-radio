use field_trait::{field_trait, field_trait_impl};

#[field_trait]
trait HasWeight {
    fn weight(&self) -> u32;
}

#[field_trait_impl()]
struct Widget {
    weight: u32,
}

#[field_trait_impl(weight = heft, HasWeight)]
struct Gadget {
    heft: u32,
}

#[field_trait_impl(HasWeight, weight = heft)]
struct Gizmo {
    weight: u32,
}

fn main() {}
