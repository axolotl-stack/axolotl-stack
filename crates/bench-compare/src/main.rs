use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;

#[global_allocator]
static GLOBAL: bench_compare::CountingAllocator = bench_compare::CountingAllocator;

#[derive(Parser)]
#[command(about = "Typed Valentine and public RakNet cross-implementation benchmarks")]
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
        Command::Codec { iterations } => bench_compare::codec::run(iterations),
        Command::Micro { iterations } => bench_compare::micro::run(iterations),
        command => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(async {
                match command {
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
