use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_nethernet::*;

// Mock Signaling logic
struct LocalSignaling {
    network_id: String,
    tx: mpsc::Sender<Signal>,
}

#[async_trait::async_trait]
impl Signaling for LocalSignaling {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        // In a real app, this sends over network.
        // Here we just pipe it to the "other side" via the tx channel.
        eprintln!("TEST: Signaling {} (type: {})", self.network_id, signal.typ);
        let _ = self.tx.send(signal).await;
        Ok(())
    }

    fn network_id(&self) -> String {
        self.network_id.clone()
    }
}

#[tokio::test]
async fn test_connect_flow() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Setup signaling channels
    // Listener -> sends signals to -> Dialer
    // Dialer -> sends signals to -> Listener

    // BUT: Listener and Dialer receive signals via their own input channels.
    // Signaling trait implementation should push to the OTHER party's input channel.

    // Listener input channel
    let (listener_sig_in_tx, listener_sig_in_rx) = mpsc::channel::<Signal>(100);
    // Dialer input channel
    let (dialer_sig_in_tx, dialer_sig_in_rx) = mpsc::channel::<Signal>(100);

    // Construct Listener
    let listener_signaling = Arc::new(LocalSignaling {
        network_id: "listener-net".to_string(),
        tx: dialer_sig_in_tx.clone(), // Listener sends to Dialer
    });

    let config = NetherNetListenerConfig::default();
    let (mut listener, listener_signal_pusher) = NetherNetListener::new(listener_signaling, config);

    // Forward signals received by listener_sig_in logic (simulating network recv) to listener
    let pusher = listener_signal_pusher.clone();
    let mut l_rx = listener_sig_in_rx;
    tokio::spawn(async move {
        while let Some(sig) = l_rx.recv().await {
            eprintln!("TEST: Forwarding signal to LISTENER: {}", sig.typ);
            let _ = pusher.send(sig).await;
        }
    });

    // Start accepting in background
    let listen_handle = tokio::spawn(async move { listener.accept().await });

    // Construct Dialer
    let dialer_signaling = Arc::new(LocalSignaling {
        network_id: "dialer-net".to_string(),
        tx: listener_sig_in_tx.clone(), // Dialer sends to Listener
    });

    let (dialer, dialer_signal_pusher) =
        NetherNetDialer::new(dialer_signaling, NetherNetDialerConfig::default());

    // Forward signals received by dialer logic to dialer
    let d_pusher = dialer_signal_pusher.clone();
    let mut d_rx = dialer_sig_in_rx;
    tokio::spawn(async move {
        while let Some(sig) = d_rx.recv().await {
            eprintln!("TEST: Forwarding signal to DIALER: {}", sig.typ);
            let _ = d_pusher.send(sig).await;
        }
    });

    // Dial!
    eprintln!("TEST: Dialing now...");
    let dial_future = dialer.dial("listener-net".to_string());

    // Wait for connection
    let (dial_res, listen_res) = tokio::join!(dial_future, listen_handle);

    let mut client_stream = dial_res.expect("Dial failed");
    let mut server_stream = listen_res.expect("Join failed").expect("Accept failed");

    println!("Connected!");

    // Test data exchange
    use futures::{SinkExt, StreamExt};

    // Client sends reliable message
    let msg = Message::reliable(bytes::Bytes::from("Hello Server"));
    client_stream.send(msg).await.expect("Client send failed");

    // Server receives
    let received = server_stream
        .next()
        .await
        .expect("Server stream closed")
        .expect("Server error");
    assert_eq!(received.buffer, "Hello Server");
    assert!(received.reliable);

    // Server sends back
    let msg = Message::unreliable(bytes::Bytes::from("Hello Client"));
    server_stream.send(msg).await.expect("Server send failed");

    // Client receives
    let received = client_stream
        .next()
        .await
        .expect("Client stream closed")
        .expect("Client error");
    assert_eq!(received.buffer, "Hello Client");
    assert!(!received.reliable);
}
