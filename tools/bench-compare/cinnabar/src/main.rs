use cinnabar_bench::{CountingAllocator, codec, micro, transport};
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
    Codec {
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
    },
    Micro {
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
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
        Command::Codec { iterations } => codec::run(iterations).map_err(Into::into),
        Command::Micro { iterations } => micro::run(iterations).map_err(Into::into),
        command => {
            let (mode, address, count) = match command {
                Command::Loopback { count } => ("loopback", "127.0.0.1:0".to_owned(), count),
                Command::Serve { address, count } => ("serve", address, count),
                Command::Client { address, count } => ("client", address, count),
                _ => unreachable!(),
            };
            let workers = std::env::var("TOKIO_WORKER_THREADS")
                .unwrap_or_else(|_| "4".into())
                .parse()?;
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .enable_all()
                .build()?
                .block_on(transport::run(mode, &address, count))
        }
    }
}
