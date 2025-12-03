/// Primitive types that map directly to simple Rust types or helper structs
#[derive(Debug, Clone, PartialEq)]
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
    McString,  // The 'pstring' or 'string' type
    Uuid,      // mcpe_uuid
    Void,      // explicitly nothing
    ByteArray, // 'restBuffer' or raw byte arrays
}

/// The core logical types in the Bedrock protocol
#[derive(Debug, Clone, PartialEq)]
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

    /// The dreaded switch (Conditional fields)
    /// Maps to a Rust Enum.
    Switch {
        compare_to: String, // The field name we are switching on (e.g. "packetId")
        fields: Vec<(String, Type)>, // "Case 1" -> Type
        default: Box<Type>, // The fallback
    },

    /// Bitmasks (maps to bitflags!)
    Bitfield {
        name: String,
        storage_type: Primitive,   // usually varint or i32
        flags: Vec<(String, u32)>, // "IsOnFire" -> 0x01
    },
}

/// Represents a Struct (Packet or nested object)
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub name: String, // "LoginPacket"
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String, // "protocol_version"
    pub type_def: Type,
}

/// A top-level Packet definition
#[derive(Debug, Clone)]
pub struct Packet {
    pub id: u32,
    pub name: String, // "Login"
    pub body: Container,
}
