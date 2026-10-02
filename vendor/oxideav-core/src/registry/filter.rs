//! Named-filter registry.
//!
//! This is the registry side of the filter pipeline: given a filter
//! name + JSON params + upstream [`PortSpec`]s, construct a
//! [`StreamFilter`] ready to wire into the pipeline. Concrete filter
//! factories (Volume, Blur, Resize, …) live in their own crates
//! (`oxideav-audio-filter`, `oxideav-image-filter`) and register
//! themselves into a [`FilterRegistry`] via their
//! `register(&mut RuntimeContext)` entry point.

use std::collections::HashMap;

use serde_json::Value;

use crate::{filter::unknown_filter_error, PortSpec, Result, StreamFilter};

/// Factory for a named filter. The registry invokes this with the
/// caller-supplied JSON `params` and the input port specs resolved
/// from upstream. Factories are free to inspect the upstream port
/// params — e.g. a resampler reads `inputs[0]` to learn the source
/// sample rate.
pub type FilterFactory =
    Box<dyn Fn(&Value, &[PortSpec]) -> Result<Box<dyn StreamFilter>> + Send + Sync>;

/// Named-filter registry. Construct with [`FilterRegistry::new`] (empty);
/// concrete filter crates populate it through their `register` entry
/// points.
#[derive(Default)]
pub struct FilterRegistry {
    factories: HashMap<String, FilterFactory>,
}

impl FilterRegistry {
    /// Empty registry — no filters resolvable until [`register`](Self::register)
    /// is called.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory under `name`. Overwrites any existing entry
    /// with the same name — last write wins.
    pub fn register(&mut self, name: &str, factory: FilterFactory) {
        self.factories.insert(name.to_string(), factory);
    }

    /// True when a filter is registered under `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    /// Instantiate the named filter. Returns an "unknown filter" error
    /// for unregistered names.
    pub fn make(
        &self,
        name: &str,
        params: &Value,
        inputs: &[PortSpec],
    ) -> Result<Box<dyn StreamFilter>> {
        let bare = strip_filter_prefix(name);
        let factory = self
            .factories
            .get(bare)
            .or_else(|| self.factories.get(name))
            .ok_or_else(|| unknown_filter_error(name))?;
        factory(params, inputs)
    }
}

/// Strip `video.`, `v:`, `audio.`, `a:` prefixes — the schema allows
/// them for disambiguation; the registry doesn't care.
fn strip_filter_prefix(name: &str) -> &str {
    name.strip_prefix("video.")
        .or_else(|| name.strip_prefix("v:"))
        .or_else(|| name.strip_prefix("audio."))
        .or_else(|| name.strip_prefix("a:"))
        .unwrap_or(name)
}
