use bytes::Bytes;
use futures::StreamExt;
use std::{error::Error, net::SocketAddr};
use tokio_raknet::{
    // We no longer need the full RaknetPacket enum at this layer
    transport::RaknetStream,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_addr: SocketAddr = std::env::var("SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:19135".to_string())
        .parse()?;

    println!("connecting to {server_addr}");

    let mut client = RaknetStream::connect(server_addr).await?;

    println!("Succesfully connected!");

    let message_payload = make_user_payload("hello server");

    client
        .send_encoded(message_payload)
        .await
        .expect("send should succeed");
    println!("client sent: hello server");

    while let Some(result) = client.next().await {
        let payload = result?;
        if let Some(text) = read_user_payload(&payload) {
            println!("client received: {text}");
            break;
        }
    }

    Ok(())
}

// Renamed and simplified to work directly with the raw payload
fn read_user_payload(payload: &Bytes) -> Option<String> {
    if payload.len() < 2 {
        return None;
    }
    // Skip the ID byte at index 0
    let slice = &payload[1..];
    let len = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    let text = String::from_utf8(slice[..len].to_vec()).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(text)
}

// Renamed and simplified to return only the raw payload
fn make_user_payload(msg: &str) -> Bytes {
    let mut data = Vec::with_capacity(1 + msg.len() + 1);
    data.push(0x80); // ID
    data.extend_from_slice(msg.as_bytes());
    data.push(0);
    // It just returns the raw bytes
    Bytes::from(data)
}
