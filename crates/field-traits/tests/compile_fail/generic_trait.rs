use field_traits::field_trait;

#[field_trait]
trait GenericTrait<T> {
    fn value(&self) -> u32;
}

fn main() {}
