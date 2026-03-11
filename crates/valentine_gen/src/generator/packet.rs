//! Packet naming helpers shared by packet emission and mcpe wrapper generation.

use crate::generator::utils::camel_case;
use proc_macro2::Ident;
use quote::format_ident;

#[derive(Debug, Clone)]
pub struct PacketNaming {
    pub variant_name: String,
    pub payload_name: String,
}

impl PacketNaming {
    pub fn variant_ident(&self) -> Ident {
        format_ident!("{}", self.variant_name)
    }

    pub fn payload_ident(&self) -> Ident {
        format_ident!("{}", self.payload_name)
    }
}

pub fn packet_naming(packet_name: &str) -> PacketNaming {
    let variant_name = camel_case(packet_name);
    let payload_name = if variant_name.ends_with("Packet") {
        variant_name.clone()
    } else if variant_name.starts_with("Packet") {
        format!("{}Packet", variant_name.trim_start_matches("Packet"))
    } else {
        format!("{}Packet", variant_name)
    };

    PacketNaming {
        variant_name,
        payload_name,
    }
}

#[cfg(test)]
mod tests {
    use super::packet_naming;

    #[test]
    fn keeps_packet_suffix_when_present() {
        let naming = packet_naming("login_packet");
        assert_eq!(naming.variant_name, "LoginPacket");
        assert_eq!(naming.payload_name, "LoginPacket");
    }

    #[test]
    fn rewrites_legacy_packet_prefix() {
        let naming = packet_naming("packet_login");
        assert_eq!(naming.variant_name, "PacketLogin");
        assert_eq!(naming.payload_name, "LoginPacket");
    }

    #[test]
    fn appends_packet_suffix_for_plain_names() {
        let naming = packet_naming("login");
        assert_eq!(naming.variant_name, "Login");
        assert_eq!(naming.payload_name, "LoginPacket");
    }
}
