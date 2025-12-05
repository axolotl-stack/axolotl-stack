use crate::ir::Type;
use proc_macro2::TokenStream;
use std::collections::{HashMap, HashSet};

pub struct GlobalRegistry {
    // Map<Fingerprint, CanonicalPath>
    // CanonicalPath: "crate::bedrock::protocol::vX_Y_Z::group::TypeName"
    pub known_types: HashMap<String, String>,
}

impl GlobalRegistry {
    pub fn new() -> Self {
        Self {
            known_types: HashMap::new(),
        }
    }

    pub fn register(&mut self, fingerprint: String, path: String) {
        self.known_types.insert(fingerprint, path);
    }

    pub fn get(&self, fingerprint: &str) -> Option<&String> {
        self.known_types.get(fingerprint)
    }
}

pub struct Context<'a> {
    pub definitions_by_group: HashMap<String, Vec<TokenStream>>,
    pub emitted: HashSet<String>,
    pub in_progress: HashSet<String>,
    pub inline_cache: HashMap<String, String>,
    pub type_lookup: HashMap<String, Type>,
    pub global_registry: &'a mut GlobalRegistry,
    pub current_module_path: String, // e.g., "crate::bedrock::protocol::v1_20_10"
    pub module_dependencies: HashSet<String>,
}
