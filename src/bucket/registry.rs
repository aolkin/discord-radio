use crate::bucket::FileResolver;
use anyhow::{Context, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A JSON document stored by name in a [`JsonRegistry`]. Its `name` field
/// holds the name it is stored under.
pub trait RegistryEntry: Serialize + DeserializeOwned {
    const KIND: &'static str;

    fn name(&self) -> &str;

    fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("names may only contain letters, digits, '-' and '_'")]
    InvalidName,
    #[error(transparent)]
    Invalid(anyhow::Error),
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
    #[error(transparent)]
    Parse(anyhow::Error),
}

pub struct JsonRegistry<T> {
    entries: RwLock<HashMap<String, Arc<T>>>,
    path: String,
    file_resolver: FileResolver,
}

impl<T: RegistryEntry> JsonRegistry<T> {
    /// Loads every entry under `path` locally and in object storage. An entry
    /// in object storage replaces a local entry of the same name.
    pub async fn from_storage(path: String, file_resolver: FileResolver) -> Self {
        let mut entries = HashMap::new();

        let filenames = file_resolver.list(&path).await;
        for filename in filenames.iter().filter(|f| f.ends_with(".json")) {
            let name = file_name_stem(filename);
            let parsed = file_resolver
                .resolve_contents(filename)
                .await
                .and_then(|bytes| parse::<T>(&bytes, name));
            match parsed {
                Ok(entry) => {
                    tracing::info!("Loaded {} {name} from {filename}", T::KIND);
                    entries.insert(name.to_string(), Arc::new(entry));
                }
                Err(e) => tracing::warn!("Failed to load {} from {filename}: {e:#}", T::KIND),
            }
        }

        if entries.is_empty() {
            tracing::warn!("No {}s loaded", T::KIND);
        } else {
            tracing::info!("Loaded {} {}s", entries.len(), T::KIND);
        }

        Self {
            entries: RwLock::new(entries),
            path,
            file_resolver,
        }
    }

    fn key(&self, name: &str) -> String {
        format!("{}{name}.json", self.path)
    }

    fn remote_key(&self, name: &str) -> Result<String, RegistryError> {
        if !is_valid_name(name) {
            return Err(RegistryError::InvalidName);
        }
        Ok(self.key(name))
    }

    pub fn get(&self, name: &str) -> Option<Arc<T>> {
        self.entries.read().unwrap().get(name).cloned()
    }

    pub fn require(&self, name: &str) -> anyhow::Result<Arc<T>> {
        self.get(name).with_context(|| {
            format!(
                "{} {name:?} ({}) is missing or invalid",
                T::KIND,
                self.key(name)
            )
        })
    }

    pub fn list(&self) -> Vec<String> {
        self.entries.read().unwrap().keys().cloned().collect()
    }

    /// Writes `body` to object storage as the entry named `name`, replacing
    /// any entry of the same name.
    pub async fn save(
        &self,
        name: &str,
        mut body: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), RegistryError> {
        let key = self.remote_key(name)?;
        body.entry("name").or_insert_with(|| name.into());
        let entry: T =
            serde_json::from_value(body.into()).map_err(|e| RegistryError::Invalid(e.into()))?;
        ensure_name(&entry, name).map_err(RegistryError::Invalid)?;
        entry.validate().map_err(RegistryError::Invalid)?;
        let bytes = serde_json::to_vec_pretty(&entry).expect("a registry entry always serializes");
        self.file_resolver
            .bucket()?
            .upload(&key, &bytes, "application/json")
            .await?;

        self.entries
            .write()
            .unwrap()
            .insert(name.to_string(), Arc::new(entry));
        Ok(())
    }

    /// Re-fetches the entry named `name` from object storage and replaces the
    /// in-memory copy with it.
    pub async fn refresh(&self, name: &str) -> Result<Arc<T>, RegistryError> {
        let key = self.remote_key(name)?;
        let bytes = self.file_resolver.bucket()?.fetch_bytes(&key).await?;
        let entry = Arc::new(parse::<T>(&bytes, name).map_err(RegistryError::Parse)?);
        self.entries
            .write()
            .unwrap()
            .insert(name.to_string(), entry.clone());
        Ok(entry)
    }
}

fn parse<T: RegistryEntry>(bytes: &[u8], name: &str) -> anyhow::Result<T> {
    let entry: T = serde_json::from_slice(bytes).context("parsing JSON")?;
    ensure_name(&entry, name)?;
    entry.validate()?;
    Ok(entry)
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn ensure_name<T: RegistryEntry>(entry: &T, name: &str) -> anyhow::Result<()> {
    if entry.name() != name {
        bail!(
            "stored name {:?} does not match file name {name:?}",
            entry.name()
        );
    }
    Ok(())
}

fn file_name_stem(filename: &str) -> &str {
    let file_name = filename.rsplit('/').next().unwrap_or(filename);
    file_name.strip_suffix(".json").unwrap_or(file_name)
}
