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
            SocketAddr::from(([10, 0, 0, 128], 0)).into(),
            10000,
            false,
            64,
            10,
            Duration::from_secs(10000),
            Duration::from_secs(1),
        )
        .unwrap(),
    ));
    pinger.start().await;
}
