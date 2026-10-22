use field_traits::{field_trait, field_trait_impl, with_trait_fields};
use serde::{Deserialize, Serialize};

#[field_trait]
pub trait Weighted {
    fn weight(&self) -> u32;
    fn name(&self) -> &str;

    fn doubled_weight(&self) -> u32 {
        self.weight() * 2
    }
}

mod traits {
    use field_traits::field_trait;

    #[field_trait]
    pub trait Volume {
        fn volume(&self) -> f32;
    }
}

#[field_trait_impl(Weighted, traits::Volume)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Track {
    weight: u32,
    name: String,
    volume: f32,
}

#[field_trait_impl(Weighted, traits::Volume)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Noise {
    weight: u32,
    name: String,
    volume: f32,
    seconds: u32,
}

#[field_trait_impl(Weighted, traits::Volume)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Entry {
    Noise(Noise),
    Track(Track),
}

#[field_trait_impl(Weighted)]
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
pub trait Kinded {
    const KIND: &'static str;
    fn priority(&self) -> u8;
    fn label(&self) -> String;
}

struct Profile {
    priority: u8,
    label: String,
}

#[field_trait_impl]
impl Kinded for Profile {
    const KIND: &'static str = "profile";

    fn priority(&self) -> u8 {
        self.priority + 1
    }
}

fn track() -> Track {
    Track {
        weight: 3,
        name: "song".to_string(),
        volume: 0.5,
    }
}

#[test]
fn struct_reads_fields() {
    let track = track();
    assert_eq!(track.weight(), 3);
    assert_eq!(track.name(), "song");
    assert_eq!(track.doubled_weight(), 6);
}

#[test]
fn enum_reads_inner_fields() {
    let entry = Entry::Noise(Noise {
        weight: 7,
        name: "static".to_string(),
        volume: 0.25,
        seconds: 10,
    });
    assert_eq!(entry.weight(), 7);
    assert_eq!(entry.name(), "static");
    assert_eq!(traits::Volume::volume(&entry), 0.25);
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
    let track = track();
    assert_eq!(track.weight(), 3);
    assert_eq!(traits::Volume::volume(&track), 0.5);
}

#[test]
fn impl_block_gets_missing_getters() {
    let profile = Profile {
        priority: 2,
        label: "loud".to_string(),
    };
    assert_eq!(Profile::KIND, "profile");
    assert_eq!(profile.priority(), 3);
    assert_eq!(profile.label(), "loud");
}

#[test]
fn serde_untagged_round_trip() {
    let entry = Entry::Track(track());
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: Entry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, entry);
    assert_eq!(parsed.weight(), 3);
}

#[with_trait_fields(Weighted)]
#[field_trait_impl(Weighted)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Jingle {
    name: String,
}

#[test]
fn with_trait_fields_adds_missing_fields() {
    let jingle = Jingle {
        name: "ding".to_string(),
        weight: 2,
    };
    assert_eq!(jingle.weight(), 2);
    assert_eq!(jingle.name(), "ding");
    let json = serde_json::to_string(&jingle).unwrap();
    assert_eq!(json, r#"{"name":"ding","weight":2}"#);
    assert_eq!(serde_json::from_str::<Jingle>(&json).unwrap(), jingle);
}
