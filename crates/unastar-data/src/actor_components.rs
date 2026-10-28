//! Typed actor component identities and bodies (D2).
//!
//! [`ActorComponent`] names every component in the pinned registry. Bodies
//! with a published stable schema are generated at build time
//! (`build/actor_components.rs`, highest stable version per component); the
//! "Dynamic value" components the runtime needs are hand-written in
//! [`dynamic`] with the same conventions: closed structs
//! (`deny_unknown_fields`), published or [`provisional`] defaults, filters kept
//! as [`FilterJson`] for `unastar_rules`, item references kept as strings.

pub(crate) mod de;
mod dynamic;
pub mod provisional;
mod range;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/actor_components.rs"));
}

pub use generated::ActorComponent;

/// A filter tree as authored (already canonicalised by the pack loader). The
/// runtime compiles it with `unastar_rules`; this is the one deliberately
/// untyped hole in component bodies.
pub type FilterJson = serde_json::Value;

/// Implemented by every typed component body.
pub trait ComponentBody: serde::de::DeserializeOwned + Clone + Send + Sync + 'static {
    const COMPONENT: ActorComponent;
}

/// One struct per component: generated from the pinned schemas plus the
/// hand-written "Dynamic value" bodies.
pub mod body {
    pub use super::dynamic::*;
    pub use super::generated::body::*;
}

/// Decode one authored body (as stored in an `ActorDefinition`) into its typed
/// form. The error names the component and the failing field path.
pub fn decode<T: ComponentBody>(body: &serde_json::Value) -> Result<T, String> {
    serde_path_to_error::deserialize(body).map_err(|error| {
        let path = error.path().to_string();
        let component = T::COMPONENT.name();
        if path == "." {
            format!("{component}: {}", error.inner())
        } else {
            format!("{component}.{path}: {}", error.inner())
        }
    })
}

/// Validation-only dispatch over every typed body: `Ok(false)` when the
/// component has no typed body, `Ok(true)` when it decoded, `Err` on a decode
/// failure.
pub fn decode_component(
    component: ActorComponent,
    body: &serde_json::Value,
) -> Result<bool, String> {
    match generated::decode_generated(component, body) {
        Some(decoded) => decoded.map(|()| true),
        None => dynamic::decode_dynamic(component, body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_written_bodies_only_cover_components_without_a_published_schema() {
        for &component in dynamic::DYNAMIC {
            assert!(
                generated::decode_generated(component, &serde_json::json!({})).is_none(),
                "{} has a generated body; drop the hand-written one",
                component.name()
            );
        }
    }
}
