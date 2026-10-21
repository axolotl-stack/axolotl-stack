//! Local HTTP NetherNet client: cargo run -p tokio-nethernet --features endpoint --example client
//! Start the listener example first. Authenticated deployments should use
//! NetherNetDialerConfig.identity with a Minecraft-issued client token.

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio_nethernet::{Message, NetherNetStream, endpoint::Client};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:19133".into());
    let mut stream = NetherNetStream::connect(address, Client::new()).await?;
    let payload = Bytes::from_static(b"Hello over NetherNet HTTP signaling");
    stream.send(Message::reliable(payload.clone())).await?;
    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await?
        .ok_or_else(|| anyhow::anyhow!("peer closed before echo"))??;
    anyhow::ensure!(reply.buffer == payload && reply.reliable, "unexpected echo");
    println!("Received reliable echo: {} bytes", reply.buffer.len());
    Ok(())
}
