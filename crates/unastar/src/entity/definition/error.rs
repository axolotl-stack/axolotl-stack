//! Typed errors of compiling definitions and composing actors from them.

use std::error::Error as StdError;

use bevy_ecs::world::error::EntityMutableFetchError;
use thiserror::Error;

/// Why one authored component body cannot become its runtime kind.
#[derive(Debug, Error)]
pub enum ComponentCompileError {
    /// The body does not decode into its generated typed struct. The text is
    /// the generated decoder's report, naming the component and field path.
    #[error("{0}")]
    Decode(String),
    /// The body decoded, but its component family rejects it (an
    /// unimplemented field, an unknown item, an unbindable filter, ...).
    #[error(transparent)]
    Rejected(Box<dyn StdError + Send + Sync>),
}

/// A component family's compile error. `?` boxes it into
/// [`ComponentCompileError::Rejected`], so a family's compile function can
/// keep its own matchable enum.
pub trait ComponentError: StdError + Send + Sync + 'static {}

impl<E: ComponentError> From<E> for ComponentCompileError {
    fn from(error: E) -> Self {
        Self::Rejected(Box::new(error))
    }
}

/// Why a component kind cannot be registered or a definition cannot compile.
#[derive(Debug, Error)]
pub enum DefinitionError {
    #[error("duplicate actor component kind {0}")]
    DuplicateKind(&'static str),
    #[error("{identifier}: no runtime implementation for component {component}")]
    UnknownComponent {
        identifier: String,
        component: String,
    },
    #[error("{identifier} / {component}: {source}")]
    Component {
        identifier: String,
        component: String,
        #[source]
        source: ComponentCompileError,
    },
}

/// Why a definition cannot be attached to an actor or its composition
/// changed. Every variant is a caller error: a missing actor or a handle
/// from another registry or definition.
#[derive(Debug, Error)]
pub enum CompositionError {
    #[error(transparent)]
    Entity(#[from] EntityMutableFetchError),
    #[error("actor already has a definition stack")]
    AlreadyAttached,
    #[error("actor has no definition stack")]
    NoStack,
    #[error("component kind belongs to another registry")]
    ForeignKind,
    #[error("component group belongs to another definition")]
    ForeignGroup,
}
