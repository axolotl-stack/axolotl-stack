//! RakNet wire capture tooling for comparing tokio-raknet with Mojang RakNet
//! (BDS, the Minecraft client). Loopback only; see
//! `tools/client-diff/raknet/README.md`.
//!
//! ```text
//! cargo run -p tokio-raknet --example raknet_capture -- relay --listen 127.0.0.1:39140 --upstream 127.0.0.1:39132 --out capture.jsonl [faults]
//! cargo run -p tokio-raknet --example raknet_capture -- probe --server 127.0.0.1:39132 --out probe.jsonl
//! cargo run -p tokio-raknet --example raknet_capture -- analyze capture.jsonl...
//! ```
//!
//! Relay faults: `--mtu N`, `--loss P` (both directions) or
//! `--loss-c2s P`/`--loss-s2c P`, `--reorder P` (or per direction),
//! `--fault-after-ms T`, `--blackhole-c2s-after-ms T`, `--seed S`.
//! The relay stops after `--duration-ms`, after `--idle-exit-ms` without
//! traffic (once traffic started), or when a line `stop` arrives on stdin.

mod analysis;
mod probe;
mod relay;
mod wire;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use relay::{Faults, Relay};

type Error = Box<dyn std::error::Error>;

struct Args {
    values: HashMap<String, String>,
    positional: Vec<String>,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Error> {
        let mut values = HashMap::new();
        let mut positional = Vec::new();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            if let Some(key) = arg.strip_prefix("--") {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--{key} needs a value"))?;
                values.insert(key.to_owned(), value);
            } else {
                positional.push(arg);
            }
        }
        Ok(Self { values, positional })
    }

    fn get<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, Error>
    where
        T::Err: std::fmt::Display,
    {
        self.values
            .get(key)
            .map(|value| {
                value
                    .parse()
                    .map_err(|error| format!("--{key} {value}: {error}").into())
            })
            .transpose()
    }

    fn required<T: std::str::FromStr>(&self, key: &str) -> Result<T, Error>
    where
        T::Err: std::fmt::Display,
    {
        self.get(key)?
            .ok_or_else(|| format!("missing --{key}").into())
    }

    fn string(&self, key: &str) -> String {
        self.values.get(key).cloned().unwrap_or_default()
    }
}

fn loopback(address: SocketAddr) -> Result<SocketAddr, Error> {
    if address.ip().is_loopback() {
        Ok(address)
    } else {
        Err(format!("{address} is not a loopback address; the capture tools never bind or relay beyond loopback").into())
    }
}

fn faults(args: &Args) -> Result<Faults, Error> {
    let loss: f64 = args.get("loss")?.unwrap_or(0.0);
    let reorder: f64 = args.get("reorder")?.unwrap_or(0.0);
    Ok(Faults {
        mtu: args.get("mtu")?,
        loss_c2s: args.get("loss-c2s")?.unwrap_or(loss),
        loss_s2c: args.get("loss-s2c")?.unwrap_or(loss),
        reorder_c2s: args.get("reorder-c2s")?.unwrap_or(reorder),
        reorder_s2c: args.get("reorder-s2c")?.unwrap_or(reorder),
        fault_after: Duration::from_millis(args.get("fault-after-ms")?.unwrap_or(0)),
        blackhole_c2s_after: args
            .get("blackhole-c2s-after-ms")?
            .map(Duration::from_millis),
        seed: args.get("seed")?.unwrap_or(0x5eed),
        ..Faults::default()
    })
}

async fn run_relay(args: &Args) -> Result<(), Error> {
    let listen = loopback(args.required("listen")?)?;
    let upstream = loopback(args.required("upstream")?)?;
    let faults = faults(args)?;
    let out: String = args.required("out")?;
    let meta = wire::Meta {
        kind: "meta".into(),
        version: 1,
        scenario: args.string("scenario"),
        server: args.string("server"),
        client: args.string("client"),
        faults: faults.describe(),
    };
    let relay = Relay::start(
        listen,
        upstream,
        faults,
        Some((std::fs::File::create(&out)?, meta)),
    )
    .await?;
    eprintln!("READY {}", relay.address);
    let duration: Option<u64> = args.get("duration-ms")?;
    let idle_exit: Option<u64> = args.get("idle-exit-ms")?;
    let started = Instant::now();
    let (stdin_stop, mut stdin_stopped) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) if line.trim() == "stop" => {
                    let _ = stdin_stop.send(());
                    return;
                }
                Ok(_) => {}
            }
        }
    });
    loop {
        tokio::select! {
            _ = &mut stdin_stopped => break,
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        if duration.is_some_and(|ms| started.elapsed() >= Duration::from_millis(ms)) {
            break;
        }
        if let Some(ms) = idle_exit
            && let Some(last) = relay.last_t_us()
            && started
                .elapsed()
                .saturating_sub(Duration::from_micros(last))
                >= Duration::from_millis(ms)
        {
            break;
        }
    }
    let records = relay.finish().await;
    eprintln!("STOPPED {} records -> {out}", records.len());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let args = Args::parse(args)?;
    match command.as_str() {
        "relay" => run_relay(&args).await,
        "probe" => {
            let server = loopback(args.required("server")?)?;
            let out: String = args.required("out")?;
            let wait = Duration::from_millis(args.get("wait-ms")?.unwrap_or(500));
            probe::run(server, &out, &args.string("server-name"), wait).await
        }
        "analyze" => {
            for path in &args.positional {
                let text = std::fs::read_to_string(path)?;
                let (meta, records) = wire::read_capture(&text)?;
                println!("# {path}\n");
                println!("{}", analysis::report(&meta, &records));
            }
            Ok(())
        }
        _ => Err("usage: raknet_capture relay|probe|analyze ... (see the module docs)".into()),
    }
}
