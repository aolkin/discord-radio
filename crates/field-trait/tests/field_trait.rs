use field_trait::{field_trait, field_trait_impl, with_trait_fields};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::sync::Arc;

#[field_trait(Arc, Rc, Box)]
pub trait HasWeight {
    fn weight(&self) -> u32;
    fn name(&self) -> &str;

    fn doubled_weight(&self) -> u32 {
        self.weight() * 2
    }
}

mod traits {
    use field_trait::field_trait;

    #[field_trait]
    pub trait HasScale {
        fn scale(&self) -> f32;
    }
}

#[field_trait_impl(HasWeight, traits::HasScale)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Widget {
    weight: u32,
    name: String,
    scale: f32,
}

#[field_trait_impl(HasWeight, traits::HasScale)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Gadget {
    weight: u32,
    name: String,
    scale: f32,
    count: u32,
}

#[field_trait_impl(HasWeight, traits::HasScale)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Item {
    Gadget(Gadget),
    Widget(Widget),
}

#[field_trait_impl(HasWeight)]
struct Wrapper<T: Clone>
where
    T: Default,
{
    weight: u32,
    name: String,
    #[allow(dead_code)]
    value: T,
}

#[field_trait(Deref)]
pub trait HasKind {
    const KIND: &'static str;
    type Rank;
    fn rank(&self) -> Self::Rank;
    fn label(&self) -> String;
}

struct Part {
    rank: u8,
    label: String,
}

#[field_trait_impl]
impl HasKind for Part {
    const KIND: &'static str = "part";
    type Rank = u8;

    fn rank(&self) -> u8 {
        self.rank + 1
    }
}

#[field_trait(Deref)]
pub trait HasParts<T: Clone, U = u8> {
    fn parts(&self) -> &Vec<T>;
    fn spare(&self) -> Option<T>;
    fn extra(&self) -> U;
}

#[field_trait_impl(HasParts<V>)]
struct Bin<V>
where
    V: Default,
{
    parts: Vec<V>,
    spare: Option<V>,
    extra: u8,
}

#[field_trait_impl(HasParts<u32>)]
enum Container {
    Bin(Bin<u32>),
}

struct Pallet {
    parts: Vec<String>,
    extra: u16,
}

#[field_trait_impl]
impl HasParts<String, u16> for Pallet {
    fn spare(&self) -> Option<String> {
        None
    }
}

fn widget() -> Widget {
    Widget {
        weight: 3,
        name: "widget".to_string(),
        scale: 0.5,
    }
}

#[test]
fn struct_reads_fields() {
    let widget = widget();
    assert_eq!(widget.weight(), 3);
    assert_eq!(widget.name(), "widget");
    assert_eq!(widget.doubled_weight(), 6);
    assert_eq!(total_weight(&Arc::new(self::widget())), 3);
    assert_eq!(total_weight(&Rc::new(self::widget())), 3);
    let boxed: Box<dyn HasWeight> = Box::new(widget);
    assert_eq!(total_weight(&boxed), 3);
    assert_eq!(HasWeight::name(&boxed), "widget");
}

fn total_weight<T: HasWeight>(item: &T) -> u32 {
    item.weight()
}

#[test]
fn enum_reads_inner_fields() {
    let item = Item::Gadget(Gadget {
        weight: 7,
        name: "gadget".to_string(),
        scale: 0.25,
        count: 10,
    });
    assert_eq!(item.weight(), 7);
    assert_eq!(item.name(), "gadget");
    assert_eq!(traits::HasScale::scale(&item), 0.25);
}

#[test]
fn generic_struct_reads_fields() {
    let wrapper = Wrapper {
        weight: 4,
        name: "wrapped".to_string(),
        value: 1u8,
    };
    assert_eq!(wrapper.weight(), 4);
    assert_eq!(wrapper.name(), "wrapped");
}

#[test]
fn multiple_traits_including_one_from_another_module() {
    let widget = widget();
    assert_eq!(widget.weight(), 3);
    assert_eq!(traits::HasScale::scale(&widget), 0.5);
}

#[test]
fn impl_block_gets_missing_getters() {
    let part = Part {
        rank: 2,
        label: "red".to_string(),
    };
    assert_eq!(Part::KIND, "part");
    assert_eq!(part.rank(), 3);
    assert_eq!(part.label(), "red");
    assert_eq!(<Arc<Part> as HasKind>::KIND, "part");
    let rank: <Arc<Part> as HasKind>::Rank = HasKind::rank(&Arc::new(part));
    assert_eq!(rank, 3u8);
}

#[test]
fn generic_trait() {
    let bin = Bin {
        parts: vec![1u32, 2],
        spare: Some(3),
        extra: 4,
    };
    assert_eq!(bin.parts(), &vec![1, 2]);
    assert_eq!(bin.spare(), Some(3));
    assert_eq!(bin.extra(), 4);
    let container = Container::Bin(bin);
    assert_eq!(container.parts(), &vec![1, 2]);
    assert_eq!(container.spare(), Some(3));
    let pallet = Pallet {
        parts: vec!["a".to_string()],
        extra: 5,
    };
    assert_eq!(pallet.parts(), &vec!["a".to_string()]);
    assert_eq!(pallet.spare(), None);
    assert_eq!(pallet.extra(), 5);
    assert_eq!(HasParts::<String, u16>::extra(&&pallet), 5);
    assert_eq!(HasParts::<String, u16>::extra(&Arc::new(pallet)), 5);
}

#[test]
fn serde_untagged_round_trip() {
    let item = Item::Widget(widget());
    let json = serde_json::to_string(&item).unwrap();
    let parsed: Item = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, item);
    assert_eq!(parsed.weight(), 3);
}

#[with_trait_fields(HasWeight)]
#[field_trait_impl(HasWeight)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Gizmo {
    name: String,
}

#[test]
fn with_trait_fields_adds_missing_fields() {
    let gizmo = Gizmo {
        name: "gizmo".to_string(),
        weight: 2,
    };
    assert_eq!(gizmo.weight(), 2);
    assert_eq!(gizmo.name(), "gizmo");
    let json = serde_json::to_string(&gizmo).unwrap();
    assert_eq!(json, r#"{"name":"gizmo","weight":2}"#);
    assert_eq!(serde_json::from_str::<Gizmo>(&json).unwrap(), gizmo);
}
