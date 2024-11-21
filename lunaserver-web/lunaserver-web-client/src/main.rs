use futures_util::{SinkExt, TryStreamExt};
use rand::{rngs::StdRng, Rng, SeedableRng};
use reqwest::Client;
use reqwest_websocket::{CloseCode, Message, RequestBuilderExt};
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    let prob = std::env::args().skip(1).next().map(|arg| arg.parse::<f64>().expect("Invalid probability"));
    let response = Client::default()
        .get("ws://lunaserver.coe.utah.edu/udp-ws")
        .upgrade() // Prepares the WebSocket upgrade.
        .send()
        .await
        .expect("Failed to connect to lunaserver");

    // Turns the response into a WebSocket stream.
    let mut websocket = response.into_websocket().await.expect("Failed to upgrade to WebSocket");
    let udp = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind UDP socket");
    udp.connect("127.0.0.1:10600").await.expect("Failed to connect to lunabase UDP socket 10600");

    let init_msg = websocket.try_next().await.expect("Failed to receive initial message from lunaserver").expect("Failed to receive initial message from lunaserver");
    if let Message::Text(text) = init_msg {
        println!("{text}")
    } else {
        panic!("Received unexpected message from lunaserver");
    }

    let mut buf = [0u8; 2048];
    let mut rng = StdRng::from_entropy();

    loop {
        tokio::select! {
            option = websocket.try_next() => {
                let Some(message) = option.expect("Failed to receive message from lunaserver") else {
                    break;
                };
                if let Message::Text(text) = message {
                    println!("{text}")
                } else if let Message::Binary(data) = message {
                    if let Some(prob) = prob {
                        if rng.gen_bool(prob)  {
                            continue;
                        }
                    }
                    // There are many non trivial reasons that the send could fail
                    let _ = udp.send(&data).await;
                }
            }
            result = udp.recv(&mut buf) => {
                let Ok(n) = result else {
                    // Also many reasons that this would fail
                    continue;
                };
                if let Some(prob) = prob {
                    if rng.gen_bool(prob)  {
                        continue;
                    }
                }
                websocket.send(Message::Binary(buf[..n].to_vec())).await.expect("Failed to send UDP packet to lunaserver");
            }
            result = tokio::signal::ctrl_c() => {
                result.expect("Failed to listen for ctrl-c");
                let _ = websocket.close(CloseCode::Normal, None).await;
                break;
            }
        }
    }
}
