use crate::ir::Type;
use proc_macro2::TokenStream;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct PacketSymbol {
    pub name: String,
    pub is_type: bool,
}

#[derive(Debug, Clone)]
pub struct PacketCanonical {
    pub packet_path: String,
    pub args_path: Option<String>,
    pub extra_symbols: Vec<PacketSymbol>,
}

pub struct GlobalRegistry {
    // Map<Fingerprint, CanonicalPath>
    // CanonicalPath: "crate::bedrock::protocol::vX_Y_Z::group::TypeName"
    pub known_types: HashMap<String, String>,
    pub known_packets: HashMap<String, PacketCanonical>,
}

impl GlobalRegistry {
    pub fn new() -> Self {
        Self {
            known_types: HashMap::new(),
            known_packets: HashMap::new(),
        }
    }

    pub fn register(&mut self, fingerprint: String, path: String) {
        self.known_types.insert(fingerprint, path);
    }

    pub fn get(&self, fingerprint: &str) -> Option<&String> {
        self.known_types.get(fingerprint)
    }

    pub fn register_packet(
        &mut self,
        fingerprint: String,
        packet_path: String,
        args_path: Option<String>,
        extra_symbols: Vec<PacketSymbol>,
    ) {
        self.known_packets.insert(
            fingerprint,
            PacketCanonical {
                packet_path,
                args_path,
                extra_symbols,
            },
        );
    }

    pub fn get_packet(&self, fingerprint: &str) -> Option<&PacketCanonical> {
        self.known_packets.get(fingerprint)
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
    /// Names of generated types whose BedrockCodec impl requires Args.
    pub argful_types: HashSet<String>,
}
