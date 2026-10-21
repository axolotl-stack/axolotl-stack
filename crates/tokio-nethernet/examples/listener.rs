//! Local HTTP NetherNet echo server: cargo run -p tokio-nethernet --features endpoint --example listener
//! The loopback-only demo explicitly permits identityless clients. Production
//! servers must configure verify_client_token and keep that opt-out disabled.

use futures::{SinkExt, StreamExt};
use p384::{ecdsa::SigningKey, elliptic_curve::rand_core::OsRng};
use std::sync::Arc;
use tokio_nethernet::{
    NetherNetListener, NetherNetListenerConfig, endpoint::Handler, identity::Identity,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let handler = Handler::serve("127.0.0.1:19133").await?;
    let config = NetherNetListenerConfig {
        identity: Some(Arc::new(Identity::generate_server(
            SigningKey::random(&mut OsRng),
            "self",
        )?)),
        allow_identityless_client: true,
        ..Default::default()
    };
    let mut listener = NetherNetListener::bind_with_signaling(handler, config);
    println!("Listening at http://127.0.0.1:19133");
    loop {
        let mut stream = listener.accept().await?;
        tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                match message {
                    Ok(message) => {
                        if let Err(error) = stream.send(message).await {
                            eprintln!("Echo send failed: {error}");
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("Peer closed: {error}");
                        break;
                    }
                }
            }
        });
    }
}
