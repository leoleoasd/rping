use pinger::Pinger;

use std::{net::SocketAddr, time::Duration};

mod pinger;

#[tokio::main]
async fn main() {
    stderrlog::new()
        .module(module_path!())
        .quiet(false)
        .verbosity(10)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    let pinger = Box::leak(Box::new(
        Pinger::new(
            SocketAddr::from(([10, 0, 0, 2], 0)).into(),
            10,
            false,
            64,
            100,
            Duration::from_secs(10000),
            Duration::from_secs(1),
        )
        .unwrap(),
    ));
    pinger.start().await;
}
