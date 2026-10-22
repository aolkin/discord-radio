use field_trait::field_trait;

#[field_trait]
trait Signature {
    fn generic<T>(&self) -> u32;
    async fn asynchronous(&self) -> u32;
    unsafe fn not_safe(&self) -> u32;
}

fn main() {}
