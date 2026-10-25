//! Inspect named structure-template records from a STOPPED Bedrock world.
//!
//! cargo run -p unastar --example bds_structure_reference -- list --stopped <world>
//! cargo run -p unastar --example bds_structure_reference -- export --stopped \
//!     --controls <capture/controls.json> <world> <outside-repo/out.json> wgbedrock:probe
//! Without controls.json, export requires --dimension overworld|nether|the_end.
//! A dimension is capture provenance, never inferred from a named database key.
//!
//! The source DB is never opened: level.dat and db/ are copied into a disposable
//! snapshot first. --stopped acknowledges that the caller has stopped its writer;
//! it is not a process detector or a substitute for stopping BDS before copying.
//! The snapshot is removed on success or error. Output is published only after
//! all selected records validate, and existing files are never overwritten.
//!
//! Key evidence (public documentation, NOT a local measurement):
//! https://minecraft.wiki/w/Bedrock_Edition_level_format documents structuretemplate.
//! https://github.com/koukiBOBS/MCBEStructureExtractor/blob/57120880a9f0085efdd922df78cdc40d86d41c16/index.html
//! uses structuretemplate_mystructure:. We therefore list the broad documented
//! prefix and export exact structuretemplate_<namespaced-name> records. A missing
//! record fails, rather than guessing alternate keys or dimensions.
//!
//! JSON schema v1 retains the complete controls document and all template NBT.
//! Typed nodes are {type,value}, with long values as decimal strings and float/
//! double values as {type,bits} (IEEE-754 hexadecimal bits, including NaNs).
//! Lists also carry element_type, null for empty lists because Reader::Tag loses
//! that declaration. raw_little_endian_hex is authoritative: it also preserves
//! compound wire order, duplicate names, and empty-list element tag IDs. No
//! unknown fields, palettes, entities, secondary blocks, or liquids are filtered.
//! Native structure format 2 retains its one or two IntArray index layers;
//! mcstructure format 1 retains both List<Int> layers. Summaries count only
//! layers actually present, without synthesizing a secondary layer.
//! Full-volume exports and original worlds must stay outside the repository.
//!
//! Shared controls schema: {seed:<i64 decimal string or JSON integer>,
//! captures:[{name:<exact namespaced name>,dimension:<canonical dimension>,
//! bounds:[x0,y0,z0,x1,y1,z1],status:"saved"|"failed",...}],...}.
//! Bounds are inclusive. A selected capture must be unique, saved, and match the
//! decoded world origin and size; seed must match level.dat. Extra metadata is
//! retained, including locate responses, query origins, and explicit failures.

#[path = "bds_structure/volume.rs"]
mod volume;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use bleveldb::{DB, Options, ReadOptions};
use serde::ser::{Serialize, SerializeSeq, Serializer};
use serde_json::Value;
use unastar_packs::nbt::{Encoding, Reader, Tag};
use volume::{Hex, Typed, Volume};

const KEY_PREFIX: &[u8] = b"structuretemplate";
const NAMED_KEY_PREFIX: &str = "structuretemplate_";
const USAGE: &str = "usage:\n  bds_structure_reference list --stopped <world>\n  bds_structure_reference export --stopped [--controls <controls.json>] [--dimension overworld|nether|the_end] <world> <outside-repo/out.json> <namespace:name>...\nStop BDS before copying. --stopped acknowledges a stopped source. Export requires controls metadata or an explicit dimension; when both are given they must agree. Output must not already exist.";

struct Args {
    command: String,
    world: PathBuf,
    output: Option<PathBuf>,
    names: Vec<String>,
    dimension: Option<String>,
    controls: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("bds_structure_reference: {error:#}");
        std::process::exit(1);
    }
}

fn parse_args() -> Result<Option<Args>> {
    let mut raw = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| anyhow::anyhow!("arguments must be valid Unicode"))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter();
    let Some(command) = raw.next() else {
        bail!("{USAGE}");
    };
    if command == "--help" || command == "-h" {
        println!("{USAGE}");
        return Ok(None);
    }
    ensure!(
        command == "list" || command == "export",
        "unknown command {command:?}\n{USAGE}"
    );
    let mut stopped = false;
    let mut dimension = None;
    let mut controls = None;
    let mut positional = Vec::new();
    let mut options = true;
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--" if options => options = false,
            "--stopped" if options => {
                ensure!(!stopped, "--stopped was supplied twice");
                stopped = true;
            }
            "--dimension" if options => {
                ensure!(dimension.is_none(), "--dimension was supplied twice");
                let value = raw.next().context("--dimension requires a value")?;
                check_dimension(&value)?;
                dimension = Some(value);
            }
            "--controls" if options => {
                ensure!(controls.is_none(), "--controls was supplied twice");
                controls = Some(PathBuf::from(
                    raw.next().context("--controls requires a path")?,
                ));
            }
            "--help" | "-h" if options => {
                println!("{USAGE}");
                return Ok(None);
            }
            _ if options && arg.starts_with('-') => bail!("unknown option {arg:?}\n{USAGE}"),
            _ => positional.push(arg),
        }
    }
    ensure!(
        stopped,
        "stop the source writer first, then acknowledge with --stopped\n{USAGE}"
    );
    if command == "list" {
        ensure!(
            positional.len() == 1 && dimension.is_none() && controls.is_none(),
            "list takes only --stopped and a world path\n{USAGE}"
        );
        return Ok(Some(Args {
            command,
            world: positional[0].clone().into(),
            output: None,
            names: vec![],
            dimension: None,
            controls: None,
        }));
    }
    ensure!(
        positional.len() >= 3,
        "export requires world, output, and at least one namespaced template name\n{USAGE}"
    );
    ensure!(
        dimension.is_some() || controls.is_some(),
        "export requires --dimension or --controls; keys do not establish source dimension"
    );
    let names = positional.split_off(2);
    let mut unique = BTreeSet::new();
    for name in &names {
        let (namespace, path) = name.split_once(':').with_context(|| {
            format!("template name {name:?} must include its persisted namespace")
        })?;
        ensure!(
            !namespace.is_empty()
                && !path.is_empty()
                && !path.contains(':')
                && !name.contains('\0'),
            "invalid namespaced template name {name:?}"
        );
        ensure!(unique.insert(name), "template {name:?} was selected twice");
    }
    Ok(Some(Args {
        command,
        world: positional[0].clone().into(),
        output: Some(positional[1].clone().into()),
        names,
        dimension,
        controls,
    }))
}

fn check_dimension(dimension: &str) -> Result<()> {
    ensure!(
        matches!(dimension, "overworld" | "nether" | "the_end"),
        "invalid dimension {dimension:?}; use overworld, nether, or the_end"
    );
    Ok(())
}

fn run() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    let source = args
        .world
        .canonicalize()
        .with_context(|| format!("resolve world {}", args.world.display()))?;
    ensure!(
        source.is_dir(),
        "world is not a directory: {}",
        source.display()
    );
    let output = args
        .output
        .as_deref()
        .map(|path| output_path(path, &source))
        .transpose()?;
    let controls = args.controls.as_deref().map(read_controls).transpose()?;
    let snapshot = tempfile::Builder::new()
        .prefix("unastar-bds-structure-reference-")
        .tempdir()
        .context("create disposable stopped-world snapshot")?;
    std::fs::copy(source.join("level.dat"), snapshot.path().join("level.dat"))
        .context("snapshot level.dat (source must be stopped)")?;
    copy_tree(&source.join("db"), &snapshot.path().join("db"))
        .context("snapshot db/ (source must be stopped)")?;
    let seed = read_seed(&snapshot.path().join("level.dat"))?;
    if let Some(controls) = &controls {
        validate_controls_seed(controls, seed)?;
    }
    let options = Options::new();
    options.create_if_missing(false);
    options.paranoid_checks(true);
    let db = DB::open(snapshot.path().join("db"), &options)
        .map_err(anyhow::Error::msg)
        .context("open disposable database snapshot")?;
    let read = ReadOptions::new();
    read.verify_checksums(true);
    read.fill_cache(false);
    if args.command == "list" {
        list_keys(&db, &read, seed)?;
    } else {
        let output = output.as_deref().expect("export output validated");
        let document = ExportDocument {
            schema_version: 1,
            kind: "bds_named_structure_volumes",
            seed: seed.to_string(),
            source_world: source.to_string_lossy().into_owned(),
            dimension_context: "controls capture metadata or explicit CLI assertion; not inferred from DB keys",
            controls: controls.as_ref(),
            structures: SelectedStructures {
                db: &db,
                read: &read,
                names: &args.names,
                dimension: args.dimension.as_deref(),
                controls: controls.as_ref(),
            },
        };
        // A bad final selection must not leave a plausible partial fixture.
        let parent = output.parent().context("output has no parent directory")?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).context("create temporary export")?;
        serde_json::to_writer(temporary.as_file_mut(), &document)
            .context("export selected structure volumes")?;
        temporary
            .as_file_mut()
            .write_all(b"\n")
            .context("finish export")?;
        temporary.as_file_mut().flush().context("flush export")?;
        temporary
            .persist_noclobber(output)
            .map_err(|error| error.error)
            .with_context(|| format!("publish export without overwriting {}", output.display()))?;
        eprintln!(
            "wrote {} complete structure volumes to {}",
            args.names.len(),
            output.display()
        );
    }
    // In particular on Windows, close the LevelDB handles before removing files.
    drop(read);
    drop(db);
    snapshot
        .close()
        .context("remove disposable structure snapshot")?;
    Ok(())
}

fn output_path(path: &Path, source: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("output must name a file")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .with_context(|| format!("resolve output directory {}", parent.display()))?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("locate workspace root")?
        .canonicalize()
        .context("resolve workspace root")?;
    ensure!(
        !parent.starts_with(&repository),
        "full-volume output must stay outside repository {}",
        repository.display()
    );
    ensure!(
        !parent.starts_with(source),
        "output must not alter the raw source world"
    );
    let output = parent.join(name);
    ensure!(
        std::fs::symlink_metadata(&output)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
        "output already exists or cannot be inspected: {}",
        output.display()
    );
    Ok(output)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    ensure!(
        source.symlink_metadata()?.file_type().is_dir(),
        "snapshot source is not a real directory: {}",
        source.display()
    );
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        } else {
            bail!(
                "unsupported snapshot entry (symlink or special file): {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn read_seed(path: &Path) -> Result<i64> {
    let bytes = std::fs::read(path).context("read snapshot level.dat")?;
    ensure!(
        bytes.len() >= 8,
        "level.dat is truncated before its eight-byte header"
    );
    let payload_length =
        u32::from_le_bytes(bytes[4..8].try_into().expect("header checked")) as usize;
    ensure!(
        payload_length == bytes.len() - 8,
        "level.dat header declares {payload_length} NBT bytes but contains {}",
        bytes.len() - 8
    );
    let mut reader = Reader::new(&bytes[8..], Encoding::LittleEndian, "snapshot level.dat");
    let (_, root) = reader.read_root().context("decode snapshot level.dat")?;
    ensure!(
        reader.is_empty(),
        "level.dat has trailing bytes after its NBT root"
    );
    match root.get("RandomSeed") {
        Some(Tag::Long(seed)) => Ok(*seed),
        _ => bail!("level.dat is missing required Long RandomSeed"),
    }
}

fn read_controls(path: &Path) -> Result<Value> {
    let value: Value = serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("read controls {}", path.display()))?,
    )
    .with_context(|| format!("parse controls {}", path.display()))?;
    ensure!(value.is_object(), "controls document must be a JSON object");
    let captures = value
        .get("captures")
        .and_then(Value::as_array)
        .context("controls must contain top-level captures array")?;
    let mut names = BTreeSet::new();
    for capture in captures {
        let name = capture
            .get("name")
            .and_then(Value::as_str)
            .context("each controls capture must have a string name")?;
        ensure!(
            names.insert(name),
            "duplicate controls capture name {name:?}"
        );
    }
    Ok(value)
}

fn validate_controls_seed(controls: &Value, seed: i64) -> Result<()> {
    let recorded = controls.get("seed").context("controls is missing seed")?;
    let recorded = match recorded {
        Value::String(value) => value
            .parse::<i64>()
            .context("controls seed must be an i64 decimal string")?,
        Value::Number(value) => value
            .as_i64()
            .context("controls seed must be an exact i64 JSON integer")?,
        _ => bail!("controls seed must be a decimal string or JSON integer"),
    };
    ensure!(
        recorded == seed,
        "controls seed {recorded} differs from snapshot seed {seed}"
    );
    Ok(())
}

fn capture_dimension<'a>(
    controls: Option<&'a Value>,
    explicit: Option<&'a str>,
    name: &str,
    volume: &Volume,
) -> Result<&'a str> {
    let Some(controls) = controls else {
        return explicit.context("dimension context is required without controls metadata");
    };
    let capture = controls["captures"]
        .as_array()
        .context("controls captures array")?
        .iter()
        .find(|capture| capture.get("name").and_then(Value::as_str) == Some(name))
        .with_context(|| format!("controls has no exact named capture {name:?}"))?;
    ensure!(
        capture.get("status").and_then(Value::as_str) == Some("saved"),
        "capture {name:?} is not status saved; failed measurements cannot be exported as success"
    );
    let dimension = capture
        .get("dimension")
        .and_then(Value::as_str)
        .with_context(|| format!("capture {name:?} is missing explicit dimension"))?;
    check_dimension(dimension)?;
    if let Some(explicit) = explicit {
        ensure!(
            explicit == dimension,
            "CLI dimension {explicit:?} differs from capture {name:?} dimension {dimension:?}"
        );
    }
    let bounds = capture
        .get("bounds")
        .and_then(Value::as_array)
        .with_context(|| {
            format!("capture {name:?} requires inclusive bounds [x0,y0,z0,x1,y1,z1]")
        })?;
    ensure!(
        bounds.len() == 6,
        "capture {name:?} bounds must contain six coordinates"
    );
    for axis in 0..3 {
        let expected_min = i64::from(volume.world_origin[axis]);
        let expected_max = expected_min + i64::from(volume.size[axis]) - 1;
        ensure!(
            bounds[axis].as_i64() == Some(expected_min)
                && bounds[axis + 3].as_i64() == Some(expected_max),
            "capture {name:?} bounds do not match decoded origin/size on axis {axis}: expected {expected_min}..={expected_max}"
        );
    }
    Ok(dimension)
}

fn list_keys(db: &DB, read: &ReadOptions, seed: i64) -> Result<()> {
    let mut iterator = db.iter(read);
    iterator.seek(KEY_PREFIX);
    let mut keys = Vec::new();
    while iterator.valid() {
        let key = iterator.key().context("valid DB iterator has no key")?;
        if !key.starts_with(KEY_PREFIX) {
            break;
        }
        // Point reads propagate read/checksum failures for every observed key.
        let bytes = db
            .get(&key, read)
            .map_err(anyhow::Error::msg)
            .context("read observed structure-template key")?
            .context("observed structure-template key disappeared from snapshot")?;
        let text = std::str::from_utf8(&key).ok();
        keys.push(serde_json::json!({
            "key": text,
            "key_hex": Hex(&key),
            "name": text.and_then(|s| s.strip_prefix(NAMED_KEY_PREFIX)),
            "value_bytes": bytes.len(),
        }));
        iterator.next_native();
    }
    // bleveldb 1.0.0 exposes no iterator status/get_error API. Do not assert a
    // complete inventory merely because valid() returned false.
    let output = serde_json::json!({
        "schema_version": 1,
        "kind": "bds_observed_structure_keys",
        "seed": seed.to_string(),
        "key_prefix": "structuretemplate",
        "dimension": null,
        "iterator_status_available": false,
        "inventory_completeness": "not certified: bleveldb does not expose iterator error status",
        "keys": keys,
    });
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer_pretty(&mut writer, &output).context("write structure-key inventory")?;
    writer.write_all(b"\n")?;
    eprintln!(
        "inventory lists observed keys only: bleveldb 1.0.0 cannot report iterator error status; selected exports use checked point reads"
    );
    Ok(())
}

#[derive(serde::Serialize)]
struct ExportDocument<'a> {
    schema_version: u32,
    kind: &'static str,
    seed: String,
    source_world: String,
    dimension_context: &'static str,
    controls: Option<&'a Value>,
    structures: SelectedStructures<'a>,
}

struct SelectedStructures<'a> {
    db: &'a DB,
    read: &'a ReadOptions,
    names: &'a [String],
    dimension: Option<&'a str>,
    controls: Option<&'a Value>,
}

#[derive(serde::Serialize)]
struct StructureExport<'a> {
    name: &'a str,
    db_key: &'a str,
    dimension: &'a str,
    format_version: i32,
    layer_count: usize,
    size: [i32; 3],
    world_origin: [i32; 3],
    block_count_per_layer: usize,
    index_order: &'static str,
    last_index_xyz: [i32; 3],
    nbt: NbtExport<'a>,
}

#[derive(serde::Serialize)]
struct NbtExport<'a> {
    encoding: &'static str,
    root_name: &'a str,
    value: Typed<'a>,
    raw_little_endian_hex: Hex<'a>,
}

impl Serialize for SelectedStructures<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let mut sequence = serializer.serialize_seq(Some(self.names.len()))?;
        for name in self.names {
            let key = format!("{NAMED_KEY_PREFIX}{name}");
            let bytes = self.db.get(key.as_bytes(), self.read)
                .map_err(|error| S::Error::custom(format!("read {key:?}: {error}")))?
                .ok_or_else(|| S::Error::custom(format!("no persisted key {key:?}; use list to inspect actual keys, and save templates to disk before stopping BDS")))?;
            let volume = Volume::read(&bytes, &key)
                .map_err(|error| S::Error::custom(format!("{error:#}")))?;
            let dimension = capture_dimension(self.controls, self.dimension, name, &volume)
                .map_err(|error| S::Error::custom(format!("{error:#}")))?;
            let last_index_xyz = volume::local_position(volume.size, volume.block_count - 1)
                .map_err(S::Error::custom)?;
            sequence.serialize_element(&StructureExport {
                name,
                db_key: &key,
                dimension,
                format_version: volume.format_version,
                layer_count: volume.layer_count,
                size: volume.size,
                world_origin: volume.world_origin,
                block_count_per_layer: volume.block_count,
                index_order: volume::INDEX_ORDER,
                last_index_xyz,
                nbt: NbtExport {
                    encoding: "little_endian",
                    root_name: &volume.root_name,
                    value: Typed(&volume.root),
                    raw_little_endian_hex: Hex(&bytes),
                },
            })?;
        }
        sequence.end()
    }
}
