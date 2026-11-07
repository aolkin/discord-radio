# field-trait

Implement getter-style traits from fields.

```rust
use field_trait::{field_trait, field_trait_impl};

#[field_trait]
pub trait HasWeight {
    fn weight(&self) -> u32;
    fn name(&self) -> &str;
}

#[field_trait_impl(HasWeight)]
struct Widget { weight: u32, name: String }

#[field_trait_impl(HasWeight)]
enum Item { Widget(Widget), Gadget(Gadget) }
```

`fn x(&self) -> T` returns `self.x.clone()` (so `T` must be `Clone`) and `fn x(&self) -> &T`
returns `&self.x`. Methods with default bodies are left alone. On an enum, every
variant must hold exactly one unnamed field, and each method reads the field from that
inner value.

On a struct or enum, `#[field_trait_impl]` takes trait names and writes each whole impl. On
a hand-written `impl Trait for Type` block, it takes no trait names and adds every getter
the block does not define, which lets a trait with associated consts or types that have
no default use it. The impl block form supports structs only, because it cannot see the
type definition:

```rust
#[field_trait_impl]
impl HasKind for Part {
    const KIND: &'static str = "part";
}
```

A `getter = field` pair makes a getter read `field` instead of the field named after the
getter. Each pair applies to the nearest trait listed before it. On an impl block, the
pairs are the only arguments and apply to the implemented trait.

```rust
#[field_trait_impl(HasWeight, weight = mass, HasScale)]
struct Sprocket { mass: u32, name: String, scale: f32 }

#[field_trait_impl(rank = level)]
impl HasKind for Part {
    const KIND: &'static str = "part";
}
```

`#[with_trait_fields(Trait, ...)]` on a struct with named fields appends a field for
each getter of the listed traits that the struct does not already declare. The field
has the getter's return type with any reference removed, and the trait's visibility. It
must be the first attribute, so the attributes below it see the added fields. This is
not inheritance: struct literals still set every field, and the added fields do not
appear in the struct's source. It takes the same `getter = field` pairs, and then
checks for and adds the mapped field instead.

```rust
#[with_trait_fields(HasWeight, weight = mass)]
#[field_trait_impl(HasWeight, weight = mass)]
#[derive(Deserialize)]
struct Gizmo { name: String }
```

A trait may have type parameters, with or without bounds and defaults, but no lifetime or
const parameters and no where clause. Name the trait with its type arguments, and the
generated impl adds the trait's bounds on its parameters to its where clause:

```rust
#[field_trait]
pub trait HasParts<T: Clone> {
    fn parts(&self) -> &Vec<T>;
}

#[field_trait_impl(HasParts<Gadget>)]
struct Toolbox { parts: Vec<Gadget> }
```

A trait implemented by `Widget` is not implemented by `Arc<Widget>`, so a generic bound
like `T: HasWeight` rejects it even though method calls on it auto-deref.
`#[field_trait(Arc, Rc, Box)]` takes a list of wrapper types and implements the trait for
each wrapper around any type that implements it, forwarding every getter to the inner
value and every associated type and const to the inner type's. A wrapper must take one
type parameter and deref to it:

```rust
#[field_trait(Arc, Box)]
pub trait HasWeight {
    fn weight(&self) -> u32;
}

fn heaviest<T: HasWeight>(items: &[T]) -> Option<u32> { ... }

heaviest(&[Arc::new(widget)]);
```

`#[field_trait(Deref)]` instead implements the trait once for every type that derefs to a
type implementing it, so it cannot be combined with wrapper names. Under that impl, a
type that implements both `Deref` and the trait directly is a conflicting impl, and so
is an impl for a type from another crate, because that crate could add a `Deref` impl
later.

`#[field_trait]` puts a hidden macro beside the trait, under the trait's name, so
importing the trait also imports what the other attributes need. Types named in the
trait's method signatures resolve where those attributes are used.
