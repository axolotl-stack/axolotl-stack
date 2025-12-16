use bytes::Bytes;
use std::{error::Error, net::SocketAddr};
use tokio_raknet::transport::{RaknetListener, RaknetStream};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:19135".to_string())
        .parse()?;

    println!("listening on {bind_addr}");
    let mut listener = RaknetListener::bind(bind_addr).await?;

    while let Some(mut conn) = listener.accept().await {
        println!(
            "[server] accept(): new connection from {}",
            conn.peer_addr()
        );
        handle_connection(&mut conn).await?;
    }

    Ok(())
}

async fn handle_connection(conn: &mut RaknetStream) -> Result<(), Box<dyn Error>> {
    println!("[server] handle_connection start peer={}", conn.peer_addr());
    while let Some(result) = conn.recv().await {
        let pkt = match result {
            Ok(p) => p,
            Err(e) => {
                println!(
                    "[server] connection error from {}: {:?}",
                    conn.peer_addr(),
                    e
                );
                break;
            }
        };
        println!("[server] recv(): got app packet from {}", conn.peer_addr());
        if let Some(text) = read_user_payload(&pkt) {
            println!("[server] received from {}: {text}", conn.peer_addr());
            let reply = make_user_payload("hello world");
            conn.send(reply).await?;
            println!(
                "[server] send(): replied with hello world to {}",
                conn.peer_addr()
            );
            break;
        }
    }

    println!("[server] handle_connection end peer={}", conn.peer_addr());

    Ok(())
}

fn read_user_payload(payload: &Bytes) -> Option<String> {
    // 1 byte ID + at least one char/null
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

fn make_user_payload(msg: &str) -> Bytes {
    let mut data = Vec::with_capacity(1 + msg.len() + 1);
    data.push(0x80); // ID
    data.extend_from_slice(msg.as_bytes());
    data.push(0);
    Bytes::from(data)
}
