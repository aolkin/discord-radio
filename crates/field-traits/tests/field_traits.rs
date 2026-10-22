use field_traits::{field_trait, field_trait_impl, with_trait_fields};
use serde::{Deserialize, Serialize};

#[field_trait]
pub trait HasWeight {
    fn weight(&self) -> u32;
    fn name(&self) -> &str;

    fn doubled_weight(&self) -> u32 {
        self.weight() * 2
    }
}

mod traits {
    use field_traits::field_trait;

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

#[field_trait]
pub trait HasKind {
    const KIND: &'static str;
    fn rank(&self) -> u8;
    fn label(&self) -> String;
}

struct Part {
    rank: u8,
    label: String,
}

#[field_trait_impl]
impl HasKind for Part {
    const KIND: &'static str = "part";

    fn rank(&self) -> u8 {
        self.rank + 1
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
