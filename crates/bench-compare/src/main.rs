use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;

#[global_allocator]
static GLOBAL: bench_compare::CountingAllocator = bench_compare::CountingAllocator;

#[derive(Parser)]
#[command(about = "Typed Valentine and public transport cross-implementation benchmarks")]
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
        address: SocketAddr,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    NethernetClient {
        #[arg(long)]
        address: SocketAddr,
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
        /// Run only RakNet wire codecs and split reassembly, excluding batch codecs.
        #[arg(long)]
        wire_only: bool,
    },
    Loopback {
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    Serve {
        #[arg(long)]
        address: SocketAddr,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    Client {
        #[arg(long)]
        address: SocketAddr,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::NethernetMicro { iterations } => bench_compare::nethernet::micro(iterations),
        Command::Codec { iterations } => bench_compare::codec::run(iterations),
        Command::Micro {
            iterations,
            wire_only,
        } => {
            bench_compare::micro::run(iterations)?;
            if wire_only {
                Ok(())
            } else {
                bench_compare::batch::run(iterations)
            }
        }
        command => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(async {
                match command {
                    Command::NethernetLoopback { count } => {
                        bench_compare::nethernet::loopback(count).await
                    }
                    Command::NethernetServe { address, count } => {
                        bench_compare::nethernet::serve(address, count).await
                    }
                    Command::NethernetClient { address, count } => {
                        bench_compare::nethernet::client(address, count).await
                    }
                    Command::Loopback { count } => bench_compare::transport::loopback(count).await,
                    Command::Serve { address, count } => {
                        bench_compare::transport::serve(address, count).await
                    }
                    Command::Client { address, count } => {
                        bench_compare::transport::client(address, count).await
                    }
                    _ => unreachable!(),
                }
            }),
    }
}
