pub mod conformance;
pub mod recording;

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use valentine::bedrock::version::v1_26_51::{McpePacketArgs, McpePacketData};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub seq: u64,
    pub t_ms: f64,
    pub dir: String,
    pub id: u32,
    pub name: String,
    pub hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Masks {
    pub timestamps: bool,
    pub ticks: bool,
    pub uuids: bool,
    pub loading_screen_ids: bool,
    pub jwt: bool,
    pub float_tolerance: f32,
    pub jwt_fields: Vec<String>,
}
impl Default for Masks {
    fn default() -> Self {
        Self {
            timestamps: true,
            ticks: true,
            uuids: true,
            loading_screen_ids: true,
            jwt: true,
            float_tolerance: 0.0001,
            jwt_fields: [
                "iat",
                "nbf",
                "exp",
                "identityPublicKey",
                "x5u",
                "cpk",
                "leguuid",
                "salt",
                "identity",
                "DeviceId",
                "ClientRandomId",
                "SelfSignedId",
                "PlayFabId",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Identical,
    MaskedEqual,
    Different,
    Missing,
}
#[derive(Debug)]
pub struct Row {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub status: Status,
}

pub fn load(path: &Path, direction: &str) -> Result<Vec<Record>> {
    ensure!(
        matches!(direction, "c2s" | "s2c"),
        "direction must be c2s or s2c"
    );
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut result = Vec::new();
    for (line, text) in text.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let record: Record = serde_json::from_str(text)
            .with_context(|| format!("{}:{}", path.display(), line + 1))?;
        ensure!(
            matches!(record.dir.as_str(), "c2s" | "s2c"),
            "invalid capture direction at line {}",
            line + 1
        );
        let data = hex::decode(&record.hex).context("invalid packet hex")?;
        let (header, _) = varint(&data).context("invalid packet header")?;
        ensure!(
            header & 1023 == record.id,
            "packet ID/header mismatch at line {}",
            line + 1
        );
        if record.dir == direction {
            result.push(record);
        }
    }
    Ok(result)
}

fn varint(data: &[u8]) -> Option<(u32, usize)> {
    let mut result = 0u32;
    for (i, byte) in data.iter().take(5).enumerate() {
        if i == 4 && *byte > 15 {
            return None;
        }
        result |= u32::from(byte & 127) << (i * 7);
        if byte & 128 == 0 {
            return Some((result, i + 1));
        }
    }
    None
}
fn write_varint(mut value: u32, output: &mut Vec<u8>) {
    while value >= 128 {
        output.push(value as u8 | 128);
        value >>= 7;
    }
    output.push(value as u8);
}
fn decode(
    data: &[u8],
) -> Option<(
    valentine::bedrock::version::v1_26_51::GameHeader,
    McpePacketData,
)> {
    let mut framed = Vec::with_capacity(data.len() + 5);
    write_varint(u32::try_from(data.len()).ok()?, &mut framed);
    framed.extend_from_slice(data);
    McpePacketData::decode_inner_bytes(&mut framed.into(), McpePacketArgs).ok()
}
fn encode(
    header: &valentine::bedrock::version::v1_26_51::GameHeader,
    data: &McpePacketData,
) -> Result<Vec<u8>> {
    let mut framed = Vec::new();
    data.encode_inner(&mut framed, header.from_subclient, header.to_subclient)?;
    let (_, prefix) = varint(&framed).context("encoder omitted frame length")?;
    Ok(framed[prefix..].to_vec())
}

fn scrub_json(value: &mut serde_json::Value, masks: &Masks) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|key, _| !masks.jwt_fields.contains(key));
            for (key, value) in map.iter_mut() {
                if key == "Certificate"
                    && let serde_json::Value::String(text) = value
                    && let Ok(mut certificate) = serde_json::from_str::<serde_json::Value>(text)
                {
                    scrub_json(&mut certificate, masks);
                    *text = certificate.to_string();
                } else {
                    scrub_json(value, masks);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_json(value, masks);
            }
        }
        serde_json::Value::String(text) => {
            if let Some(normalized) = scrub_jwt(text, masks) {
                *text = normalized;
            }
        }
        _ => {}
    }
}
fn scrub_jwt(token: &str, masks: &Masks) -> Option<String> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).ok()?).ok()?;
    let mut body: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).ok()?).ok()?;
    scrub_json(&mut header, masks);
    scrub_json(&mut body, masks);
    Some(format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).ok()?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&body).ok()?)
    ))
}
fn scrub_login(data: &[u8], masks: &Masks) -> Option<Vec<u8>> {
    let mut cursor = data;
    let mut result = Vec::new();
    for is_jwt in [false, true] {
        let len = u32::from_le_bytes(cursor.get(..4)?.try_into().ok()?) as usize;
        let text = std::str::from_utf8(cursor.get(4..4usize.checked_add(len)?)?).ok()?;
        let normalized = if is_jwt {
            scrub_jwt(text, masks)?
        } else {
            let mut value = serde_json::from_str(text).ok()?;
            scrub_json(&mut value, masks);
            serde_json::to_string(&value).ok()?
        };
        result.extend_from_slice(&u32::try_from(normalized.len()).ok()?.to_le_bytes());
        result.extend_from_slice(normalized.as_bytes());
        cursor = cursor.get(4 + len..)?;
    }
    if !cursor.is_empty() {
        return None;
    }
    Some(result)
}
fn normalize(packet: &mut McpePacketData, masks: &Masks) {
    use McpePacketData::*;
    match packet {
        LoginPacket(p) if masks.jwt => {
            if let Some(data) = scrub_login(&p.connection_request, masks) {
                p.connection_request = data.into();
            }
        }
        ServerToClientHandshakePacket(p) if masks.jwt => {
            if let Some(token) = scrub_jwt(&p.handshake_web_token, masks) {
                p.handshake_web_token = token.into();
            }
        }
        NetworkStackLatencyPacket(p) if masks.timestamps => p.creation_time = 0,
        MovePlayerPacket(p) if masks.ticks => p.tick.inputtick = 0,
        PlayerAuthInputPacket(p) if masks.ticks => p.client_tick.inputtick = 0,
        UpdateAttributesPacket(p) if masks.ticks => p.tick.inputtick = 0,
        SetActorDataPacket(p) if masks.ticks => p.tick.inputtick = 0,
        SetActorMotionPacket(p) if masks.ticks => p.tick.inputtick = 0,
        CommandRequestPacket(p) if masks.uuids => {
            p.origin.uuid = Default::default();
            p.origin.request_id = Default::default();
        }
        ServerboundLoadingScreenPacket(p)
            if masks.loading_screen_ids && p.loading_screen_id.is_some() =>
        {
            p.loading_screen_id = Some(0);
        }
        _ => {}
    }
}
fn close(a: f32, b: &mut f32, tolerance: f32) {
    if a.is_finite() && b.is_finite() && (a - *b).abs() <= tolerance {
        *b = a;
    }
}
fn vec3(
    a: &valentine::bedrock::version::v1_26_51::Vec3,
    b: &mut valentine::bedrock::version::v1_26_51::Vec3,
    t: f32,
) {
    close(a.x, &mut b.x, t);
    close(a.y, &mut b.y, t);
    close(a.z, &mut b.z, t);
}
fn vec2(
    a: &valentine::bedrock::version::v1_26_51::Vec2,
    b: &mut valentine::bedrock::version::v1_26_51::Vec2,
    t: f32,
) {
    close(a.x, &mut b.x, t);
    close(a.y, &mut b.y, t);
}
fn jitter(a: &McpePacketData, b: &mut McpePacketData, t: f32) {
    use McpePacketData::*;
    match (a, b) {
        (MovePlayerPacket(a), MovePlayerPacket(b)) => {
            vec3(&a.position, &mut b.position, t);
            vec2(&a.rotation, &mut b.rotation, t);
            close(a.y_head_rotation, &mut b.y_head_rotation, t);
        }
        (PlayerAuthInputPacket(a), PlayerAuthInputPacket(b)) => {
            vec3(&a.position, &mut b.position, t);
            vec3(&a.pos_delta, &mut b.pos_delta, t);
            vec3(&a.camera_orientation, &mut b.camera_orientation, t);
            vec2(&a.player_rotation, &mut b.player_rotation, t);
            vec2(&a.move_vector, &mut b.move_vector, t);
            vec2(&a.interact_rotation, &mut b.interact_rotation, t);
            vec2(&a.analog_move_vector, &mut b.analog_move_vector, t);
            vec2(&a.raw_move_vector, &mut b.raw_move_vector, t);
            close(a.player_head_rotation, &mut b.player_head_rotation, t);
        }
        _ => {}
    }
}
fn status(a: &Record, b: &Record, masks: &Masks) -> Result<Status> {
    let a = hex::decode(&a.hex)?;
    let b = hex::decode(&b.hex)?;
    if a == b {
        return Ok(Status::Identical);
    }
    let (Some((ah, mut ap)), Some((bh, mut bp))) = (decode(&a), decode(&b)) else {
        return Ok(Status::Different);
    };
    // Codec canonicalization is not a configured mask. Preserve nonminimal
    // varints, noncanonical booleans, and lossy string representations as raw.
    if encode(&ah, &ap)? != a || encode(&bh, &bp)? != b {
        return Ok(Status::Different);
    }
    normalize(&mut ap, masks);
    normalize(&mut bp, masks);
    jitter(&ap, &mut bp, masks.float_tolerance);
    Ok(if encode(&ah, &ap)? == encode(&bh, &bp)? {
        Status::MaskedEqual
    } else {
        Status::Different
    })
}

pub fn compare(a: &[Record], b: &[Record], masks: &Masks) -> Result<Vec<Row>> {
    ensure!(
        masks.float_tolerance.is_finite() && masks.float_tolerance >= 0.,
        "float tolerance must be finite and nonnegative"
    );
    // LCS aligns packet IDs; ties preserve earliest matching packets in the left log.
    let cols = b.len() + 1;
    let cells = (a.len() + 1)
        .checked_mul(cols)
        .context("alignment size overflow")?;
    ensure!(
        cells <= 25_000_000,
        "capture too large for exact alignment; select a shorter scenario/direction"
    );
    let mut lcs = vec![0u32; cells];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * cols + j] = if a[i].id == b[j].id {
                1 + lcs[(i + 1) * cols + j + 1]
            } else {
                lcs[(i + 1) * cols + j].max(lcs[i * cols + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut rows = Vec::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i].id == b[j].id {
            rows.push(Row {
                left: Some(i),
                right: Some(j),
                status: status(&a[i], &b[j], masks)?,
            });
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i * cols + j + 1] >= lcs[(i + 1) * cols + j])
        {
            rows.push(Row {
                left: None,
                right: Some(j),
                status: Status::Missing,
            });
            j += 1;
        } else {
            rows.push(Row {
                left: Some(i),
                right: None,
                status: Status::Missing,
            });
            i += 1;
        }
    }
    Ok(rows)
}

pub fn report(a: &[Record], b: &[Record], rows: &[Row]) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::from(
        "# Packet sequence diff\n\n| Left seq | Right seq | ID | Packet | Result |\n|---:|---:|---:|---|---|\n",
    );
    let mut counts = [0usize; 4];
    for row in rows {
        let left = row.left.map(|i| &a[i]);
        let right = row.right.map(|i| &b[i]);
        let packet = left.or(right).context("empty alignment row")?;
        counts[match row.status {
            Status::Identical => 0,
            Status::MaskedEqual => 1,
            Status::Different => 2,
            Status::Missing => 3,
        }] += 1;
        writeln!(
            out,
            "| {} | {} | {} | {} | {:?} |",
            left.map_or("-".into(), |p| p.seq.to_string()),
            right.map_or("-".into(), |p| p.seq.to_string()),
            packet.id,
            packet.name,
            row.status
        )?;
    }
    out.push_str("\n# Packet byte differences\n\nDiffering bytes are bracketed; absent bytes are `--`. Dumps show original wire bytes, not normalized bytes.\n\n");
    for row in rows {
        let left = row.left.map(|i| &a[i]);
        let right = row.right.map(|i| &b[i]);
        let packet = left.or(right).context("empty row")?;
        let ab = hex::decode(left.map_or("", |p| p.hex.as_str()))?;
        let bb = hex::decode(right.map_or("", |p| p.hex.as_str()))?;
        writeln!(
            out,
            "## {} ({}) — {:?}\n\n```text\n offset  left                                              right",
            packet.name, packet.id, row.status
        )?;
        for offset in (0..ab.len().max(bb.len())).step_by(16) {
            let mut sides = [String::new(), String::new()];
            for k in offset..(offset + 16).min(ab.len().max(bb.len())) {
                for (side, data) in [ab.as_slice(), bb.as_slice()].into_iter().enumerate() {
                    let value = data.get(k).map_or("--".into(), |v| format!("{v:02x}"));
                    if ab.get(k) != bb.get(k) {
                        write!(sides[side], "[{value}]")?;
                    } else {
                        write!(sides[side], " {value} ")?;
                    }
                }
            }
            writeln!(out, "{offset:07x}  {:64}  {}", sides[0], sides[1])?;
        }
        out.push_str("```\n\n");
    }
    writeln!(
        out,
        "# Summary\n\n| Identical | Masked-equal | Different | Missing |\n|---:|---:|---:|---:|\n| {} | {} | {} | {} |",
        counts[0], counts[1], counts[2], counts[3]
    )?;
    Ok(out)
}
