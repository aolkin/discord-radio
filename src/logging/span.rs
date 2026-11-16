macro_rules! guild_span {
    (root $name:literal, $guild_id:expr) => {
        tracing::info_span!(parent: None, $name, guild_id = u64::from($guild_id))
    };
    ($name:literal, $guild_id:expr) => {
        tracing::info_span!($name, guild_id = u64::from($guild_id))
    };
}

pub(crate) use guild_span;
