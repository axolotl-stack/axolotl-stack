//! The `01_server` and `02_client` examples joined against each other over
//! real RakNet loopback, so the Tier 1 examples keep compiling and working.

#![cfg(all(feature = "server", feature = "client", feature = "raknet"))]

use std::time::Duration;

use jolyne::{ClientHandshakeConfig, WorldTemplate};
use p384::SecretKey;

// Each example's `main` (and the Xbox Live login) is not called here.
#[allow(dead_code)]
#[path = "../examples/01_server.rs"]
mod server;

#[allow(dead_code)]
#[path = "../examples/02_client.rs"]
mod client;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn example_client_joins_example_server() {
    let mut listener = server::bind("127.0.0.1:0", false).await.unwrap();
    let addr = listener.local_addr().unwrap();
    // One iteration of `server::serve`'s accept loop, awaited so the test
    // sees what the server saw.
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap();
        let server_key = SecretKey::random(&mut rand::thread_rng());
        let chats = server::handle_player(stream, &WorldTemplate::default(), &server_key).await;
        // The listener lives until the player is done.
        (chats, listener)
    });

    tokio::time::timeout(
        Duration::from_secs(30),
        client::run_client(addr, ClientHandshakeConfig::random(addr, "Smoke")),
    )
    .await
    .expect("example client timed out")
    .expect("example client joined, chatted and disconnected");

    let (chats, _listener) = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("example server saw the disconnect")
        .unwrap();
    assert_eq!(chats.expect("example server ran the player"), 1);
}
