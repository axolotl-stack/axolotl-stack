use cinnabar_bench::{CountingAllocator, codec, micro, nethernet, transport};
use clap::{Parser, Subcommand};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[derive(Parser)]
#[command(about = "Isolated Cinnabar public API and protocol contract benchmarks")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    NethernetMicro {
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
    },
    NethernetLoopback {
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    NethernetServe {
        #[arg(long)]
        address: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    NethernetClient {
        #[arg(long)]
        address: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    Codec {
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
    },
    Micro {
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
        /// Select only RakNet wire-codec rows, excluding batch codecs.
        #[arg(long)]
        wire_only: bool,
    },
    Loopback {
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    Serve {
        #[arg(long)]
        address: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    Client {
        #[arg(long)]
        address: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse().command {
        Command::NethernetMicro { iterations } => nethernet::micro(iterations).map_err(Into::into),
        Command::Codec { iterations } => codec::run(iterations).map_err(Into::into),
        Command::Micro {
            iterations,
            wire_only,
        } => micro::run(iterations, wire_only).map_err(Into::into),
        command => {
            let (mode, address, count) = match command {
                Command::Loopback { count } => ("loopback", "127.0.0.1:0".to_owned(), count),
                Command::Serve { address, count } => ("serve", address, count),
                Command::Client { address, count } => ("client", address, count),
                Command::NethernetLoopback { count } => {
                    ("nethernet-loopback", "127.0.0.1:0".to_owned(), count)
                }
                Command::NethernetServe { address, count } => ("nethernet-serve", address, count),
                Command::NethernetClient { address, count } => ("nethernet-client", address, count),
                _ => unreachable!(),
            };
            let workers = std::env::var("TOKIO_WORKER_THREADS")
                .unwrap_or_else(|_| "4".into())
                .parse()?;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .enable_all()
                .build()?;
            if let Some(mode) = mode.strip_prefix("nethernet-") {
                runtime
                    .block_on(nethernet::run(mode, &address, count))
                    .map_err(Into::into)
            } else {
                runtime.block_on(transport::run(mode, &address, count))
            }
        }
    }
}
