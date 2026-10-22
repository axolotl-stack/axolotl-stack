//! Resource pack negotiation and transfer, both directions.
//!
//! Vanilla ordering (gophertunnel `conn.go`, BDS captures):
//!
//! 1. server → `ResourcePacksInfo`
//! 2. client → `Downloading{ids}` (or `DownloadingFinished` when it has
//!    everything, or `Cancel`)
//! 3. per requested pack: server → `ResourcePackDataInfo`, client →
//!    `ResourcePackChunkRequest{i}` in order, server → `ResourcePackChunkData`
//! 4. client → `DownloadingFinished`; only now server → `ResourcePackStack`
//! 5. client → `ResourcePackStackFinished`
//!
//! Memory is bounded on both sides: the server only slices shared pack
//! content; the client caps the pack count and the advertised per-pack and
//! total sizes, requires `ResourcePackDataInfo` to match the advertised size,
//! grows its buffer only as chunks arrive, keeps at most `window` chunk
//! requests in flight, and verifies every downloaded pack's SHA-256. Cache hits
//! are size-checked; the cache is trusted for content.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::conn::Connection;
use crate::error::{JolyneError, PackError};
use crate::valentine::{
    ContentIdentity, Experiments, McpePacket, PackIdVersion, PackIdVersionjson, PackInfoData,
    PackInstanceId, ResourcePackChunkDataPacket, ResourcePackChunkRequestPacket,
    ResourcePackClientResponseDownloadingFinishedjson, ResourcePackClientResponseDownloadingjson,
    ResourcePackClientResponsePacket, ResourcePackClientResponsePacketResponse,
    ResourcePackClientResponseResourcePackStackFinishedjson, ResourcePackDataInfoPacket,
    ResourcePackStackPacket, ResourcePacksInfoPacket, SemVersion, SemVersionjson,
};

/// gophertunnel `DefaultResourcePackChunkSize`.
pub const DEFAULT_CHUNK_SIZE: u32 = 128 * 1024;
/// gophertunnel `DefaultResourcePackChunkSendDelay`.
pub const DEFAULT_CHUNK_INTERVAL: Duration = Duration::from_millis(200);

/// Built-in vanilla packs clients list in stacks without downloading them
/// (gophertunnel `exemptedPacks`, protocol 2193).
pub const BUILTIN_PACKS: &[(&str, &str)] = &[
    ("d34cfa4b-2ad1-453d-a0db-668b429a3ea0", "1.26.40"),
    ("b41c2785-c512-4a49-af56-3a87afd47c57", "1.21.30"),
    ("a4df0cb3-17be-4163-88d7-fcf7002b935d", "1.21.20"),
    ("d19adffe-a2e1-4b02-8436-ca4583368c89", "1.21.10"),
    ("85d5603d-2824-4b21-8044-34f441f4fce1", "1.21.0"),
    ("e977cd13-0a11-4618-96fb-03dfe9c43608", "1.20.60"),
    ("0674721c-a0aa-41a1-9ba8-1ed33ea3e7ed", "1.20.50"),
    ("0fba4063-dba1-4281-9b89-ff9390653530", "1.0.0"),
];

fn is_builtin(uuid: &str, version: &str) -> bool {
    BUILTIN_PACKS
        .iter()
        .any(|&(id, v)| id.eq_ignore_ascii_case(uuid) && v == version)
}

/// `ResourcePackDataInfo.pack_type` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PackType {
    Addon = 1,
    Cached = 2,
    CopyProtected = 3,
    Behaviour = 4,
    PersonaPiece = 5,
    #[default]
    Resources = 6,
    Skins = 7,
    WorldTemplate = 8,
}

/// A pack the server can send: identity plus its `.mcpack`/zip bytes.
/// Content and hash are private so they always agree and are never empty.
#[derive(Debug, Clone)]
pub struct ResourcePack {
    pub uuid: Uuid,
    pub version: String,
    content: Bytes,
    sha256: [u8; 32],
    pub pack_type: PackType,
    /// AES key for encrypted (marketplace-style) packs; empty when plain.
    pub content_key: String,
    pub sub_pack_name: String,
    /// CDN URL clients may fetch the pack from instead of in-band chunks.
    pub cdn_url: String,
    pub has_scripts: bool,
    pub is_addon: bool,
    pub ray_tracing: bool,
}

impl ResourcePack {
    /// Hashes `content` once; the content is shared, never copied, by transfers.
    pub fn new(uuid: Uuid, version: impl Into<String>, content: Bytes) -> Result<Self, PackError> {
        let mut pack = Self::new_unhashed(uuid, version.into(), content)?;
        pack.sha256 = Sha256::digest(&pack.content).into();
        Ok(pack)
    }

    /// Wraps content whose SHA-256 was already verified (a proxied download).
    #[cfg(all(feature = "server", feature = "client"))]
    pub(crate) fn from_verified(
        uuid: Uuid,
        version: String,
        content: Bytes,
        sha256: [u8; 32],
    ) -> Result<Self, PackError> {
        let mut pack = Self::new_unhashed(uuid, version, content)?;
        pack.sha256 = sha256;
        Ok(pack)
    }

    fn new_unhashed(uuid: Uuid, version: String, content: Bytes) -> Result<Self, PackError> {
        let name = format!("{uuid}_{version}");
        if content.is_empty() {
            return Err(PackError::Invalid(name, "empty content"));
        }
        if version.is_empty() {
            return Err(PackError::Invalid(name, "empty version"));
        }
        Ok(Self {
            uuid,
            version,
            sha256: [0; 32],
            content,
            pack_type: PackType::Resources,
            content_key: String::new(),
            sub_pack_name: String::new(),
            cdn_url: String::new(),
            has_scripts: false,
            is_addon: false,
            ray_tracing: false,
        })
    }

    pub fn content(&self) -> &Bytes {
        &self.content
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    /// The `uuid_version` name used in requests and chunk packets.
    pub fn name(&self) -> String {
        format!("{}_{}", self.uuid, self.version)
    }

    fn info(&self) -> PackInfoData {
        PackInfoData {
            pack_id_version: PackIdVersion {
                pack_uuid: self.uuid,
                pack_version: SemVersion {
                    version: self.version.clone().into(),
                },
            },
            pack_size: self.content.len() as u64,
            content_key: self.content_key.clone().into(),
            subpack_name: self.sub_pack_name.clone().into(),
            content_identity: ContentIdentity {
                identity: if self.content_key.is_empty() {
                    String::new().into()
                } else {
                    self.uuid.to_string().into()
                },
            },
            has_scripts: self.has_scripts,
            is_addon_pack: self.is_addon,
            is_ray_tracing_capable: self.ray_tracing,
            cdnurl: self.cdn_url.clone().into(),
        }
    }
}

/// Packs a server offers, and how it paces chunks.
#[derive(Debug, Clone)]
pub struct ServerPacks {
    pub packs: Vec<Arc<ResourcePack>>,
    /// Clients must accept the packs to join.
    pub required: bool,
    pub chunk_size: u32,
    /// Minimum spacing between chunk replies; zero disables pacing.
    /// Pacing is a timer, never a sleep: other packets keep flowing.
    pub chunk_interval: Duration,
    pub force_disable_vibrant_visuals: bool,
}

impl Default for ServerPacks {
    fn default() -> Self {
        Self {
            packs: Vec::new(),
            required: false,
            chunk_size: DEFAULT_CHUNK_SIZE,
            chunk_interval: DEFAULT_CHUNK_INTERVAL,
            force_disable_vibrant_visuals: false,
        }
    }
}

impl ServerPacks {
    pub fn len(&self) -> usize {
        self.packs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    fn find(&self, name: &str) -> Option<&Arc<ResourcePack>> {
        let (uuid, version) = name.split_once('_').unwrap_or((name, ""));
        self.packs.iter().find(|pack| {
            pack.uuid.to_string().eq_ignore_ascii_case(uuid)
                && (version.is_empty() || pack.version == version)
        })
    }
}

fn response(response: ResourcePackClientResponsePacketResponse) -> McpePacket {
    McpePacket::from(ResourcePackClientResponsePacket { response })
}

/// Result of feeding one packet to a pack exchange.
#[derive(Debug, PartialEq, Eq)]
pub enum PackProgress {
    /// The packet belonged to the exchange; keep going.
    Handled,
    /// The exchange is finished.
    Done,
    /// Not a pack packet; the caller decides (usually defers it).
    NotMine,
}

struct Upload {
    pack: Arc<ResourcePack>,
    name: String,
    chunks: u32,
    next: u32,
}

/// The server half of the exchange.
pub struct ServerPackExchange {
    packs: Arc<ServerPacks>,
    queue: VecDeque<Arc<ResourcePack>>,
    upload: Option<Upload>,
    /// Chunk requests accepted but held back by pacing.
    held: VecDeque<u32>,
    next_chunk_at: Option<Instant>,
    stack_sent: bool,
}

impl ServerPackExchange {
    /// Queues `ResourcePacksInfo`.
    pub fn start(
        packs: Arc<ServerPacks>,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<Self, JolyneError> {
        let info = ResourcePacksInfoPacket {
            resource_pack_required: packs.required,
            has_addon_packs: packs.packs.iter().any(|pack| pack.is_addon),
            has_scripts: packs.packs.iter().any(|pack| pack.has_scripts),
            force_disable_vibrant_visuals: packs.force_disable_vibrant_visuals,
            world_template_id_and_version: PackIdVersionjson {
                pack_uuid: Uuid::nil(),
                pack_version: SemVersionjson {
                    version: String::new().into(),
                },
            },
            resource_packs: packs.packs.iter().map(|pack| pack.info()).collect(),
        };
        conn.queue_packet(&McpePacket::from(info), now)?;
        conn.flush()?;
        Ok(Self {
            packs,
            queue: VecDeque::new(),
            upload: None,
            held: VecDeque::new(),
            next_chunk_at: None,
            stack_sent: false,
        })
    }

    /// When the next held chunk may be sent.
    pub fn timer(&self) -> Option<Instant> {
        (!self.held.is_empty())
            .then_some(self.next_chunk_at)
            .flatten()
    }

    fn send_data_info(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        let Some(pack) = self.queue.pop_front() else {
            return Ok(());
        };
        let chunk_size = self.packs.chunk_size.max(1);
        let chunks = pack.content.len().div_ceil(chunk_size as usize) as u32;
        let name = pack.name();
        conn.queue_packet(
            &McpePacket::from(ResourcePackDataInfoPacket {
                resource_name: name.clone().into(),
                chunk_size,
                numberof_chunks: chunks,
                file_size: pack.content.len() as u64,
                file_hash: Bytes::copy_from_slice(&pack.sha256),
                is_premium_pack: false,
                pack_type: pack.pack_type as u8,
            }),
            now,
        )?;
        conn.flush()?;
        self.upload = Some(Upload {
            pack,
            name,
            chunks,
            next: 0,
        });
        Ok(())
    }

    fn send_chunk(
        &mut self,
        index: u32,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let upload = self
            .upload
            .as_mut()
            .expect("chunk without an active upload");
        let size = self.packs.chunk_size.max(1) as usize;
        let start = index as usize * size;
        let end = (start + size).min(upload.pack.content.len());
        conn.queue_packet(
            &McpePacket::from(ResourcePackChunkDataPacket {
                resource_name: upload.name.clone().into(),
                chunk_id: index,
                byte_offset: start as u64,
                chunk_data: upload.pack.content.slice(start..end),
            }),
            now,
        )?;
        conn.flush()?;
        if !self.packs.chunk_interval.is_zero() {
            self.next_chunk_at = Some(now + self.packs.chunk_interval);
        }
        if index + 1 == upload.chunks {
            self.upload = None;
            self.send_data_info(conn, now)?;
        }
        Ok(())
    }

    /// Sends held chunks whose pacing slot has arrived.
    pub fn on_timer(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        while let Some(&index) = self.held.front() {
            if self.next_chunk_at.is_some_and(|at| now < at) {
                break;
            }
            self.held.pop_front();
            self.send_chunk(index, conn, now)?;
        }
        Ok(())
    }

    fn send_stack(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        let stack = ResourcePackStackPacket {
            texture_pack_required: self.packs.required,
            texture_pack_list: self
                .packs
                .packs
                .iter()
                .filter(|pack| pack.pack_type != PackType::Behaviour)
                .map(|pack| PackInstanceId {
                    pack_id: pack.uuid.to_string().into(),
                    version: pack.version.clone().into(),
                    sub_pack_name: pack.sub_pack_name.clone().into(),
                })
                .collect(),
            base_game_version: crate::valentine::GAME_VERSION.into(),
            experiments: Experiments {
                toggles: vec![],
                experiments_ever_toggled: false,
            },
            include_editor_packs: false,
        };
        conn.queue_packet(&McpePacket::from(stack), now)?;
        conn.flush()?;
        self.stack_sent = true;
        Ok(())
    }

    pub fn on_packet(
        &mut self,
        packet: &McpePacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<PackProgress, JolyneError> {
        use crate::valentine::McpePacketData as D;
        use ResourcePackClientResponsePacketResponse as R;
        match &packet.data {
            D::ResourcePackClientResponsePacket(packet) => match &packet.response {
                R::Cancel(_) if self.packs.required => Err(PackError::Refused.into()),
                R::Cancel(_) | R::DownloadingFinished(_) => {
                    if !self.stack_sent {
                        self.send_stack(conn, now)?;
                    }
                    Ok(PackProgress::Handled)
                }
                R::Downloading(request) => {
                    for name in &request.downloading_packs {
                        let pack = self
                            .packs
                            .find(name)
                            .ok_or_else(|| PackError::UnknownPack(name.to_string()))?;
                        if !self.queue.iter().any(|queued| Arc::ptr_eq(queued, pack)) {
                            self.queue.push_back(pack.clone());
                        }
                    }
                    if self.upload.is_none() {
                        self.send_data_info(conn, now)?;
                    }
                    Ok(PackProgress::Handled)
                }
                R::ResourcePackStackFinished(_) if self.stack_sent => Ok(PackProgress::Done),
                R::ResourcePackStackFinished(_) => Err(PackError::Invalid(
                    "stack".into(),
                    "stack finished before the stack was sent",
                )
                .into()),
            },
            D::ResourcePackChunkRequestPacket(request) => {
                let name = request.resource_name.to_string();
                let Some(upload) = self.upload.as_mut() else {
                    return Err(PackError::UnexpectedChunk {
                        pack: name,
                        reason: "no transfer in progress",
                    }
                    .into());
                };
                let (uuid, _) = name.split_once('_').unwrap_or((&name, ""));
                if !upload.pack.uuid.to_string().eq_ignore_ascii_case(uuid) {
                    return Err(PackError::UnknownPack(name).into());
                }
                let index =
                    u32::try_from(request.chunk).map_err(|_| PackError::ChunkOutOfRange {
                        pack: name.clone(),
                        index: 0,
                        count: upload.chunks,
                    })?;
                if index >= upload.chunks {
                    return Err(PackError::ChunkOutOfRange {
                        pack: name,
                        index,
                        count: upload.chunks,
                    }
                    .into());
                }
                if index != upload.next {
                    return Err(PackError::ChunkOutOfOrder {
                        pack: name,
                        index,
                        expected: upload.next,
                    }
                    .into());
                }
                upload.next += 1;
                self.held.push_back(index);
                self.on_timer(conn, now)?;
                Ok(PackProgress::Handled)
            }
            _ => Ok(PackProgress::NotMine),
        }
    }
}

/// Information a server advertised about one pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOffer {
    pub uuid: Uuid,
    pub version: String,
    pub size: u64,
    pub content_key: String,
    pub sub_pack_name: String,
    pub cdn_url: String,
    pub has_scripts: bool,
    pub is_addon: bool,
}

impl PackOffer {
    pub fn name(&self) -> String {
        format!("{}_{}", self.uuid, self.version)
    }
}

/// A pack the client downloaded and verified.
#[derive(Debug, Clone)]
pub struct DownloadedPack {
    pub offer: PackOffer,
    pub content: Bytes,
    pub sha256: [u8; 32],
}

/// Optional on-disk/in-memory cache for downloaded packs.
pub trait PackCache: Send + Sync {
    /// Returns cached content for `offer`, if present. The content is
    /// size-checked before use.
    fn load(&self, offer: &PackOffer) -> Option<Bytes>;
    fn store(&self, pack: &DownloadedPack);
}

/// Decides per offer whether to download it (gophertunnel
/// `DownloadResourcePack`). Declined packs are treated as present when the
/// stack references them.
pub type PackFilter = Arc<dyn Fn(&PackOffer) -> bool + Send + Sync>;

/// Client download policy and bounds.
#[derive(Clone)]
pub struct ClientPackPolicy {
    /// Whether to download offered packs at all.
    pub download: bool,
    pub filter: Option<PackFilter>,
    pub cache: Option<Arc<dyn PackCache>>,
    pub max_packs: usize,
    pub max_pack_size: u64,
    pub max_total_size: u64,
    /// Chunk requests kept in flight (1 = gophertunnel's one-at-a-time).
    pub window: u32,
}

impl Default for ClientPackPolicy {
    fn default() -> Self {
        Self {
            download: true,
            filter: None,
            cache: None,
            max_packs: 256,
            max_pack_size: 512 * 1024 * 1024,
            max_total_size: 1024 * 1024 * 1024,
            window: 4,
        }
    }
}

impl std::fmt::Debug for ClientPackPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientPackPolicy")
            .field("download", &self.download)
            .field("filter", &self.filter.is_some())
            .field("cache", &self.cache.is_some())
            .field("max_packs", &self.max_packs)
            .field("max_pack_size", &self.max_pack_size)
            .field("max_total_size", &self.max_total_size)
            .field("window", &self.window)
            .finish()
    }
}

struct Download {
    offer: PackOffer,
    name: String,
    chunk_size: u32,
    chunks: u32,
    size: u64,
    hash: Bytes,
    buffer: BytesMut,
    requested: u32,
    received: u32,
}

/// The client half of the exchange.
pub struct ClientPackExchange {
    policy: ClientPackPolicy,
    required: bool,
    offers: Vec<PackOffer>,
    ignored: Vec<PackOffer>,
    awaiting: Vec<PackOffer>,
    current: Option<Download>,
    downloaded: Vec<DownloadedPack>,
    info_seen: bool,
    finished_sent: bool,
}

impl ClientPackExchange {
    pub fn new(policy: ClientPackPolicy) -> Self {
        Self {
            policy,
            required: false,
            offers: Vec::new(),
            ignored: Vec::new(),
            awaiting: Vec::new(),
            current: None,
            downloaded: Vec::new(),
            info_seen: false,
            finished_sent: false,
        }
    }

    /// Downloaded packs and whether the server marked them required.
    pub fn into_downloaded(self) -> (Vec<DownloadedPack>, bool) {
        (self.downloaded, self.required)
    }

    fn finish_downloads(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        if self.awaiting.is_empty() && self.current.is_none() && !self.finished_sent {
            self.finished_sent = true;
            conn.queue_packet(
                &response(
                    ResourcePackClientResponsePacketResponse::DownloadingFinished(
                        ResourcePackClientResponseDownloadingFinishedjson {
                            response_type: "downloadingfinished".into(),
                        },
                    ),
                ),
                now,
            )?;
            conn.flush()?;
        }
        Ok(())
    }

    fn on_info(
        &mut self,
        info: &ResourcePacksInfoPacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<(), JolyneError> {
        if self.info_seen {
            return Ok(());
        }
        self.info_seen = true;
        self.required = info.resource_pack_required;
        if info.resource_packs.len() > self.policy.max_packs {
            return Err(PackError::TooManyPacks {
                count: info.resource_packs.len(),
                max: self.policy.max_packs,
            }
            .into());
        }
        let mut total = 0u64;
        for pack in &info.resource_packs {
            let offer = PackOffer {
                uuid: pack.pack_id_version.pack_uuid,
                version: pack.pack_id_version.pack_version.version.to_string(),
                size: pack.pack_size,
                content_key: pack.content_key.to_string(),
                sub_pack_name: pack.subpack_name.to_string(),
                cdn_url: pack.cdnurl.to_string(),
                has_scripts: pack.has_scripts,
                is_addon: pack.is_addon_pack,
            };
            if self.offers.iter().any(|seen| seen.uuid == offer.uuid) {
                continue;
            }
            self.offers.push(offer.clone());
            let wanted = self.policy.download
                && self
                    .policy
                    .filter
                    .as_ref()
                    .is_none_or(|filter| filter(&offer));
            if !wanted {
                self.ignored.push(offer);
                continue;
            }
            if let Some(content) = self
                .policy
                .cache
                .as_ref()
                .and_then(|cache| cache.load(&offer))
                .filter(|content| content.len() as u64 == offer.size)
            {
                self.downloaded.push(DownloadedPack {
                    sha256: Sha256::digest(&content).into(),
                    content,
                    offer,
                });
                continue;
            }
            if offer.size > self.policy.max_pack_size {
                return Err(PackError::PackTooLarge {
                    pack: offer.name(),
                    size: offer.size,
                    max: self.policy.max_pack_size,
                }
                .into());
            }
            total = total.saturating_add(offer.size);
            if total > self.policy.max_total_size {
                return Err(PackError::TotalTooLarge {
                    max: self.policy.max_total_size,
                }
                .into());
            }
            self.awaiting.push(offer);
        }
        if self.awaiting.is_empty() {
            return self.finish_downloads(conn, now);
        }
        conn.queue_packet(
            &response(ResourcePackClientResponsePacketResponse::Downloading(
                ResourcePackClientResponseDownloadingjson {
                    response_type: "downloading".into(),
                    downloading_packs: self.awaiting.iter().map(|o| o.name().into()).collect(),
                },
            )),
            now,
        )?;
        conn.flush()
    }

    fn request_more(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        let window = self.policy.window.max(1);
        let Some(download) = self.current.as_mut() else {
            return Ok(());
        };
        let mut sent = false;
        while download.requested < download.chunks
            && download.requested - download.received < window
        {
            conn.queue_packet(
                &McpePacket::from(ResourcePackChunkRequestPacket {
                    resource_name: download.name.clone().into(),
                    chunk: download.requested as i32,
                }),
                now,
            )?;
            download.requested += 1;
            sent = true;
        }
        if sent {
            conn.flush()?;
        }
        Ok(())
    }

    fn on_data_info(
        &mut self,
        info: &ResourcePackDataInfoPacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let name = info.resource_name.to_string();
        if self.current.is_some() {
            return Err(PackError::UnexpectedChunk {
                pack: name,
                reason: "data info while another pack is transferring",
            }
            .into());
        }
        let (uuid, version) = name.split_once('_').unwrap_or((&name, ""));
        let position = self
            .awaiting
            .iter()
            .position(|offer| {
                offer.uuid.to_string().eq_ignore_ascii_case(uuid)
                    && (version.is_empty() || offer.version == version)
            })
            .ok_or_else(|| PackError::UnknownPack(name.clone()))?;
        if info.chunk_size == 0 {
            return Err(PackError::InvalidChunkSize {
                pack: name,
                size: 0,
            }
            .into());
        }
        // The advertised size already passed the per-pack and total caps.
        if info.file_size != self.awaiting[position].size {
            return Err(PackError::Invalid(name, "size differs from ResourcePacksInfo").into());
        }
        // Like gophertunnel, derive the chunk count from the size rather than
        // trusting `numberof_chunks`.
        let chunks =
            u32::try_from(info.file_size.div_ceil(u64::from(info.chunk_size))).map_err(|_| {
                PackError::InvalidChunkSize {
                    pack: name.clone(),
                    size: info.chunk_size,
                }
            })?;
        let offer = self.awaiting.remove(position);
        let initial = (u64::from(info.chunk_size) * u64::from(self.policy.window.max(1)))
            .min(info.file_size) as usize;
        self.current = Some(Download {
            offer,
            name,
            chunk_size: info.chunk_size,
            chunks,
            size: info.file_size,
            hash: info.file_hash.clone(),
            buffer: BytesMut::with_capacity(initial),
            requested: 0,
            received: 0,
        });
        if chunks == 0 {
            return self.complete(conn, now);
        }
        self.request_more(conn, now)
    }

    fn complete(&mut self, conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
        let download = self.current.take().expect("completing without a download");
        let sha256: [u8; 32] = Sha256::digest(&download.buffer).into();
        // Deliberately stricter than gophertunnel, which never checks the
        // hash: the advertised value must be the 32-byte SHA-256 and match.
        if download.hash.len() != 32 || download.hash[..] != sha256 {
            return Err(PackError::HashMismatch(download.name).into());
        }
        let pack = DownloadedPack {
            offer: download.offer,
            content: download.buffer.freeze(),
            sha256,
        };
        if let Some(cache) = &self.policy.cache {
            cache.store(&pack);
        }
        self.downloaded.push(pack);
        self.finish_downloads(conn, now)
    }

    fn on_chunk(
        &mut self,
        chunk: &ResourcePackChunkDataPacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let name = chunk.resource_name.to_string();
        let Some(download) = self.current.as_mut().filter(|d| d.name == name) else {
            return Err(PackError::UnexpectedChunk {
                pack: name,
                reason: "no transfer for this pack",
            }
            .into());
        };
        if chunk.chunk_id != download.received || chunk.chunk_id >= download.requested {
            return Err(PackError::UnexpectedChunk {
                pack: name,
                reason: "chunk out of order or not requested",
            }
            .into());
        }
        let offset = u64::from(chunk.chunk_id) * u64::from(download.chunk_size);
        let expected_len = (download.size - offset).min(u64::from(download.chunk_size));
        if chunk.byte_offset != offset || chunk.chunk_data.len() as u64 != expected_len {
            return Err(PackError::UnexpectedChunk {
                pack: name,
                reason: "chunk offset or length mismatch",
            }
            .into());
        }
        download.buffer.extend_from_slice(&chunk.chunk_data);
        download.received += 1;
        if download.received == download.chunks {
            return self.complete(conn, now);
        }
        self.request_more(conn, now)
    }

    fn on_stack(
        &mut self,
        stack: &ResourcePackStackPacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<(), JolyneError> {
        for entry in &stack.texture_pack_list {
            let (uuid, version) = (entry.pack_id.to_string(), entry.version.to_string());
            let known = is_builtin(&uuid, &version)
                || self
                    .ignored
                    .iter()
                    .chain(self.downloaded.iter().map(|pack| &pack.offer))
                    .any(|offer| {
                        offer.uuid.to_string().eq_ignore_ascii_case(&uuid)
                            && offer.version == version
                    });
            if !known {
                return Err(PackError::MissingStackPack(format!("{uuid}_{version}")).into());
            }
        }
        conn.queue_packet(
            &response(
                ResourcePackClientResponsePacketResponse::ResourcePackStackFinished(
                    ResourcePackClientResponseResourcePackStackFinishedjson {
                        response_type: "resourcepackstackfinished".into(),
                    },
                ),
            ),
            now,
        )?;
        conn.flush()
    }

    pub fn on_packet(
        &mut self,
        packet: &McpePacket,
        conn: &mut Connection,
        now: Instant,
    ) -> Result<PackProgress, JolyneError> {
        use crate::valentine::McpePacketData as D;
        match &packet.data {
            D::ResourcePacksInfoPacket(info) => self.on_info(info, conn, now)?,
            D::ResourcePackDataInfoPacket(info) => self.on_data_info(info, conn, now)?,
            D::ResourcePackChunkDataPacket(chunk) => self.on_chunk(chunk, conn, now)?,
            D::ResourcePackStackPacket(stack) => {
                if self.current.is_some() || !self.awaiting.is_empty() {
                    return Err(PackError::Invalid(
                        "stack".into(),
                        "stack received before downloads finished",
                    )
                    .into());
                }
                self.on_stack(stack, conn, now)?;
                return Ok(PackProgress::Done);
            }
            _ => return Ok(PackProgress::NotMine),
        }
        Ok(PackProgress::Handled)
    }
}

#[cfg(test)]
mod tests;
