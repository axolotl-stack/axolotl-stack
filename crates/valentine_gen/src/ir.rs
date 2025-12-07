/// Primitive types that map directly to simple Rust types or helper structs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Primitive {
    Bool,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    VarInt,    // Maps to i32, encoded as varint
    VarLong,   // Maps to i64, encoded as varlong
    ZigZag32,  // i32 encoded via zigzag varint
    ZigZag64,  // i64 encoded via zigzag varlong
    McString,  // The 'pstring' or 'string' type
    Uuid,      // mcpe_uuid
    Void,      // explicitly nothing
    ByteArray, // 'restBuffer' or raw byte arrays
}

/// The core logical types in the Bedrock protocol
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Type {
    /// A simple primitive (e.g., "count": "varint")
    Primitive(Primitive),

    /// A reference to a named type defined elsewhere (e.g., "type": "Item")
    /// The generator will treat this as a struct name.
    Reference(String),

    /// A nested anonymous struct (common in Bedrock)
    Container(Container),

    /// An array of items
    Array {
        count_type: Box<Type>, // Usually Type::Primitive(VarInt)
        inner_type: Box<Type>, // The thing inside the array
    },

    /// Optional value
    Option(Box<Type>),

    /// The dreaded switch (Conditional fields)
    /// Maps to a Rust Enum.
    Switch {
        compare_to: String, // The field name we are switching on (e.g. "packetId")
        fields: Vec<(String, Type)>, // "Case 1" -> Type
        default: Box<Type>, // The fallback
    },

    /// Numeric discriminator mapped to named variants (e.g., mapper { type: varint, mappings: {...} })
    /// This becomes a C-like Rust enum with explicit discriminants.
    Enum {
        underlying: Primitive,
        variants: Vec<(String, i64)>,
    },

    /// Bitmasks (maps to bitflags!)
    Bitfield {
        name: String,
        storage_type: Primitive,   // usually varint or i32
        flags: Vec<(String, u64)>, // "IsOnFire" -> bitmask value
    },

    // A primitive (u8, u16) that contains bit-packed sub-variables
    Packed {
        backing: Primitive,       // e.g. U8
        fields: Vec<PackedField>, // e.g. "type" -> mask 0x07
    },
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct PackedField {
    pub name: String,
    pub shift: u32,
    pub mask: u64,
}

/// Represents a Struct (Packet or nested object)
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Container {
    pub name: String, // "LoginPacket"
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Field {
    pub name: String, // "protocol_version"
    pub type_def: Type,
}

/// A top-level Packet definition
#[derive(Debug, Clone, Hash)]
pub struct Packet {
    pub id: u32,
    pub name: String, // "Login"
    pub body: Container,
}
