//! Durable world manifests and exclusive provider lifetimes for declarative startup.

use crate::config::{UnastarConfig, WorldStorageConfig};
use crate::storage::{ChunkColumn, StorageResult, WorldProvider};
use crate::world::{
    ChunkPos, StorageProvider, WorldBounds, WorldConfig, WorldGenerator, WorldIdentity,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
};
use uuid::Uuid;

const MANIFEST: &str = "world.toml";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Attach validated identity and load-before-generate storage before plugins or players exist.
pub(super) fn install(
    server: &mut crate::server::GameServer,
    storage: &WorldStorageConfig,
    world: WorldConfig,
) -> io::Result<Uuid> {
    let (identity, provider) = open(storage, world)?;
    let id = identity.id;
    server
        .ecs
        .world_mut()
        .resource_mut::<crate::world::ecs::ChunkManager>()
        .set_provider(provider);
    server.ecs.world_mut().insert_resource(identity);
    Ok(id)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terrain {
    dimension: i32,
    bounds: WorldBounds,
    generator: WorldGenerator,
    storage_provider: StorageProvider,
}
impl From<WorldConfig> for Terrain {
    fn from(world: WorldConfig) -> Self {
        Self {
            dimension: world.dimension,
            bounds: world.bounds,
            generator: world.generator,
            storage_provider: world.storage_provider,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    id: String,
    name: String,
    terrain: Terrain,
}

/// Resolve aliases before any provider opens, including nested database directories.
pub(super) fn validate_directories(config: &UnastarConfig) -> io::Result<()> {
    let mut paths = Vec::new();
    for path in std::iter::once(&config.world_storage.data_dir)
        .chain(config.secondary_worlds.iter().map(|world| &world.data_dir))
        .chain(
            config
                .players
                .leveldb_enabled
                .then_some(&config.players.data_dir),
        )
    {
        fs::create_dir_all(path)?;
        let canonical = fs::canonicalize(path)?;
        reject_database_symlink(&canonical)?;
        if paths.iter().any(|prior: &std::path::PathBuf| {
            prior.starts_with(&canonical) || canonical.starts_with(prior)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "world and player storage directories must be disjoint",
            ));
        }
        paths.push(canonical);
    }
    Ok(())
}

/// Open only after metadata validation; failures never disable persistence and regenerate terrain.
pub(super) fn open(
    storage: &WorldStorageConfig,
    world: WorldConfig,
) -> io::Result<(WorldIdentity, Arc<dyn WorldProvider>)> {
    fs::create_dir_all(&storage.data_dir)?;
    let root = fs::canonicalize(&storage.data_dir)?;
    reject_database_symlink(&root)?;
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".world.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lease)?;
    let path = root.join(MANIFEST);
    let terrain = Terrain::from(world);
    let manifest = match File::open(&path) {
        Ok(file) => {
            let mut text = String::new();
            file.take(MAX_MANIFEST_BYTES + 1)
                .read_to_string(&mut text)?;
            if text.len() as u64 > MAX_MANIFEST_BYTES {
                return Err(invalid("world manifest exceeds its size limit"));
            }
            let manifest: Manifest = toml_edit::de::from_str(&text).map_err(invalid)?;
            if manifest.version != 1 || manifest.name != storage.name || manifest.terrain != terrain
            {
                return Err(invalid(
                    "world identity or terrain settings differ from the saved manifest; restore the original configuration",
                ));
            }
            manifest
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let database = root.join("db");
            let existing = match fs::read_dir(&database) {
                Ok(mut entries) => entries.next().transpose()?.is_some(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(error),
            };
            if existing && !storage.adopt_existing {
                return Err(invalid(
                    "existing world data has no manifest; verify its original terrain settings and explicitly enable adopt_existing",
                ));
            }
            let manifest = Manifest {
                version: 1,
                id: Uuid::new_v4().to_string(),
                name: storage.name.clone(),
                terrain,
            };
            persist_manifest(&root, &path, &manifest)?;
            manifest
        }
        Err(error) => return Err(error),
    };
    let id = Uuid::parse_str(&manifest.id).map_err(invalid)?;
    if id.is_nil() {
        return Err(invalid("world manifest UUID cannot be nil"));
    }
    let path = root.join("db");
    let provider: Arc<dyn WorldProvider> = match world.storage_provider {
        StorageProvider::LevelDb => Arc::new(
            crate::storage::LevelDBWorldProvider::open(&path, world.dimension).map_err(invalid)?,
        ),
        StorageProvider::BlazeDb => crate::storage::BlazeDBProvider::open(
            &path,
            Some(crate::storage::blazedb::BlazeConfig {
                cache_capacity: world.blazedb_cache_chunks,
                ..Default::default()
            }),
        )
        .map_err(invalid)?,
    };
    let lease = Arc::new(lease);
    if let Some(writer) = provider.column_writer() {
        writer.attach_lease(lease.clone()).map_err(invalid)?;
    }
    Ok((
        WorldIdentity {
            id,
            name: manifest.name,
            dimension: world.dimension,
        },
        Arc::new(LeasedProvider {
            inner: provider,
            _lease: lease,
        }),
    ))
}

/// Publish a synced manifest without ever replacing an existing valid record.
fn persist_manifest(root: &Path, path: &Path, manifest: &Manifest) -> io::Result<()> {
    let pending = root.join(format!(".world-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        file.write_all(
            toml_edit::ser::to_string(manifest)
                .map_err(invalid)?
                .as_bytes(),
        )?;
        file.sync_all()?;
        fs::hard_link(&pending, path)?;
        fs::remove_file(&pending)?;
        File::open(root)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}

/// Keep the exclusive metadata lease while any asynchronous provider user survives.
struct LeasedProvider {
    inner: Arc<dyn WorldProvider>,
    _lease: Arc<File>,
}
#[async_trait]
impl WorldProvider for LeasedProvider {
    async fn load_column(
        &self,
        pos: ChunkPos,
        dimension: i32,
    ) -> StorageResult<Option<ChunkColumn>> {
        self.inner.load_column(pos, dimension).await
    }
    async fn save_column(
        &self,
        pos: ChunkPos,
        dimension: i32,
        column: &ChunkColumn,
    ) -> StorageResult<()> {
        self.inner.save_column(pos, dimension, column).await
    }
    fn column_writer(&self) -> Option<Arc<crate::storage::column_writer::ColumnWriter>> {
        self.inner.column_writer()
    }
    async fn flush(&self) -> StorageResult<()> {
        self.inner.flush().await
    }
    async fn close(&self) -> StorageResult<()> {
        self.inner.close().await
    }
}

/// Persisted metadata is untrusted input and never silently resets a world.
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Database aliases must use their actual parent directory so one lease guards one store.
fn reject_database_symlink(root: &Path) -> io::Result<()> {
    match fs::symlink_metadata(root.join("db")) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "database directory cannot be a symlink; select its actual parent directory",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
