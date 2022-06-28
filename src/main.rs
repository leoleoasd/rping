use clap::{ArgAction, Parser, ValueEnum};
use log::trace;
use pinger::Pinger;

use std::net::{Ipv4Addr, SocketAddr};

mod pinger;

#[derive(Parser, Debug)]
#[clap(version)]
struct Cli {
    #[clap(short, long, help = "add more v to get more detailed output", action = ArgAction::Count)]
    verbosity: u8,
    #[clap(short, long, help = "disable log output", action = ArgAction::SetTrue)]
    quiet: bool,
    #[clap(long, value_enum, default_value = "off")]
    timestamp: Timestamp,
    #[clap(help = "host to ping")]
    host: Ipv4Addr,
    #[clap(
        short,
        long,
        help = "number of pings to send, use -1 for infinite",
        default_value = "-1"
    )]
    count: i16,
    #[clap(short, long)]
    broadcast: bool,
    #[clap(short, long, help = "time between pings", default_value = "1s")]
    interval: humantime::Duration,
    #[clap(short, long, default_value = "128")]
    ttl: u8,
    #[clap(short, long, default_value = "32")]
    size: u16,
    #[clap(short='r', long="route", help = "Don't use the system routing table", action = ArgAction::SetTrue)]
    route: bool,
    #[clap(long, help = "Timeout for each ping", default_value = "5s")]
    timeout: humantime::Duration,
}

#[derive(Clone, ValueEnum, Debug)]
enum Timestamp {
    #[clap(alias("none"))]
    Off,
    #[clap(aliases(["s", "sec"]))]
    Second,
    #[clap(alias("ms"))]
    Millisecond,
    #[clap(alias("us"))]
    Microsecond,
    #[clap(alias("ns"))]
    Nanosecond,
}
impl From<Timestamp> for stderrlog::Timestamp {
    fn from(timestamp: Timestamp) -> Self {
        match timestamp {
            Timestamp::Off => stderrlog::Timestamp::Off,
            Timestamp::Second => stderrlog::Timestamp::Second,
            Timestamp::Millisecond => stderrlog::Timestamp::Millisecond,
            Timestamp::Microsecond => stderrlog::Timestamp::Microsecond,
            Timestamp::Nanosecond => stderrlog::Timestamp::Nanosecond,
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    stderrlog::new()
        .module(module_path!())
        .quiet(args.quiet)
        .verbosity((args.verbosity + 2) as usize)
        .timestamp(args.timestamp.clone().into())
        .init()
        .unwrap();
    trace!("args = {args:?}");

    let pinger = Box::leak(Box::new(
        Pinger::new(
            SocketAddr::from((args.host, 0)).into(),
            if args.count >= 0 {
                args.count as u16
            } else {
                u16::MAX
            },
            args.broadcast,
            args.size,
            args.ttl,
            args.timeout.into(),
            args.interval.into(),
            args.route,
        )
        .unwrap(),
    ));
    pinger.start().await;
}
