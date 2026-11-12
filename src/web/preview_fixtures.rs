use crate::audio::dj::config::{DJConfig, Pool};
use crate::audio::profiles::SignalProfile;
use crate::web::routes::discord::{GuildsResponse, SendableChannelsResponse};
use crate::web::routes::files::BrowseFilesResponse;
use crate::web::state_snapshot::BotSnapshot;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn contains(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected
            .iter()
            .all(|(key, value)| actual.get(key).is_some_and(|a| contains(a, value))),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(a, e)| contains(a, e))
        }
        _ => actual == expected,
    }
}

fn check<T: DeserializeOwned + Serialize>(path: &str, json: &str) {
    let expected: Value =
        serde_json::from_str(json).unwrap_or_else(|e| panic!("{path} is not JSON: {e:#}"));
    let parsed: T = serde_json::from_value(expected.clone())
        .unwrap_or_else(|e| panic!("{path} does not match its type: {e:#}"));
    let actual = serde_json::to_value(&parsed).unwrap();
    assert!(
        contains(&actual, &expected),
        "{path} changes when it is parsed and written back:\n{actual:#}"
    );
}

macro_rules! fixture {
    ($ty:ty, $path:literal) => {
        check::<$ty>(
            $path,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/preview/fixtures/",
                $path
            )),
        )
    };
}

#[test]
fn preview_fixtures_round_trip() {
    fixture!(GuildsResponse, "guilds.json");
    fixture!(SendableChannelsResponse, "channels.json");
    fixture!(BrowseFilesResponse, "files/tracks.json");
    fixture!(BotSnapshot, "snapshots/playing-track.json");
    fixture!(BotSnapshot, "snapshots/playing-hex.json");
    fixture!(BotSnapshot, "snapshots/playing-noise.json");
    fixture!(BotSnapshot, "snapshots/no-radio.json");
    fixture!(SignalProfile, "profiles/clear.json");
    fixture!(SignalProfile, "profiles/weak_signal.json");
    fixture!(SignalProfile, "profiles/tuning.json");
    fixture!(Pool, "pools/track.json");
    fixture!(Pool, "pools/hex.json");
    fixture!(Pool, "pools/noise.json");
    fixture!(DJConfig, "dj-configs/default.json");
    fixture!(Value, "bot-profile.json");
}
