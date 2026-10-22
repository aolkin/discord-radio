use field_traits::field_trait;

#[field_trait]
trait ReturnType {
    fn nothing(&self);
    fn mutable(&self) -> &mut u32;
}

fn main() {}
