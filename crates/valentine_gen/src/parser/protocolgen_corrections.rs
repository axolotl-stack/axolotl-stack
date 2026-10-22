//! Corrections for protocolgen canonical-manifest defects that captured BDS
//! traffic disproves.
//!
//! protocolgen is pinned as a submodule and must not be patched in place, so
//! the frontend replaces the affected field types while lowering. Each
//! correction is keyed on the packet and the protocolgen field name and pins
//! the exact canonical node it replaces: if upstream changes that node (for
//! example after fixing the defect) generation fails instead of silently
//! applying a stale correction. A correction that matches no field of a
//! manifest for the protocol version it was verified against is an error
//! too. Upstream reports live in `crates/valentine_gen/PROTOCOLGEN_ISSUES.md`.

use crate::ir::Primitive;
use std::collections::HashSet;

/// Replaces one packet field's primitive with the wire type BDS actually uses.
pub(super) struct FieldCorrection {
    pub packet: &'static str,
    pub field: &'static str,
    /// protocolgen primitive code the manifest must still carry.
    pub expected_code: &'static str,
    /// protocolgen `encoding` the manifest must still carry.
    pub expected_encoding: &'static str,
    pub replacement: Primitive,
    /// Protocol version whose captured traffic proves the correction; its
    /// manifest must contain the corrected field.
    pub verified_protocol: i32,
}

/// Field corrections, each backed by captured BDS packets.
pub(super) const FIELD_CORRECTIONS: &[FieldCorrection] = &[
    // LevelEventGenericPacket event data is a `CompoundTag` written without a
    // root header: named tags followed by TAG_End, ending the packet. The
    // manifest types it as one rooted network NBT value, so decoding read the
    // first child tag as the "root" and left the remaining children as
    // trailing packet bytes. Verified on every captured event ID (sleeping
    // players 9801, record playback 1901/1902, sculk 2026-2038); see the
    // fixtures in `crates/valentine/tests/fixtures/conformance`.
    FieldCorrection {
        packet: "LevelEventGenericPacket",
        field: "__[[CTD]]__",
        expected_code: "nbt_le",
        expected_encoding: "network",
        replacement: Primitive::NbtCompoundBody,
        verified_protocol: 2193,
    },
];

/// Looks up the correction for a packet field and checks the manifest node it
/// replaces is unchanged.
pub(super) fn correction_for(
    packet: &str,
    field: &str,
    kind: &str,
    code: Option<&str>,
    encoding: Option<&str>,
    applied: &mut HashSet<(&'static str, &'static str)>,
) -> Result<Option<Primitive>, String> {
    let Some(correction) = FIELD_CORRECTIONS
        .iter()
        .find(|correction| correction.packet == packet && correction.field == field)
    else {
        return Ok(None);
    };
    if kind != "primitive"
        || code != Some(correction.expected_code)
        || encoding != Some(correction.expected_encoding)
    {
        return Err(format!(
            "protocolgen correction for {packet}.{field:?} expects a {} ({}) primitive, but the manifest now has kind {kind:?}, code {code:?}, encoding {encoding:?}; re-verify against captures and update or remove the correction",
            correction.expected_code, correction.expected_encoding
        ));
    }
    applied.insert((correction.packet, correction.field));
    Ok(Some(correction.replacement.clone()))
}

/// Fails when a correction matched no field of a manifest for the protocol
/// version it was verified against (the field was renamed or removed).
pub(super) fn ensure_all_applied(
    protocol_version: i32,
    applied: &HashSet<(&'static str, &'static str)>,
) -> Result<(), String> {
    let stale: Vec<_> = FIELD_CORRECTIONS
        .iter()
        .filter(|correction| {
            correction.verified_protocol == protocol_version
                && !applied.contains(&(correction.packet, correction.field))
        })
        .map(|correction| format!("{}.{:?}", correction.packet, correction.field))
        .collect();
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "protocolgen corrections matched no manifest field: {}",
            stale.join(", ")
        ))
    }
}
