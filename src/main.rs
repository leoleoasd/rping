use pinger::Pinger;
use clap::{Parser, Args, Subcommand, ValueEnum, ArgAction};

use std::{net::{SocketAddr, Ipv4Addr}, time::Duration, str::FromStr};

mod pinger;

#[derive(Parser, Debug)]
#[clap(version)]
struct Cli {
    #[clap(short, long, help = "add more v to get more detailed output", action = ArgAction::Count)]
    verbosity: u8,
    #[clap(short, long, help = "disable log output", action = ArgAction::SetTrue)]
    quiet: bool,
    #[clap(short, long, value_enum, default_value = "off")]
    timestamp: Timestamp,
    #[clap(help = "host to ping")]
    host: Ipv4Addr,
    #[clap(short, long, help = "number of pings to send, use -1 for infinite", default_value = "-1")]
    count: i16,
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
    println!("{:?}", args);
    stderrlog::new()
        .module(module_path!())
        .quiet(args.quiet)
        .verbosity((args.verbosity + 2) as usize)
        .timestamp(args.timestamp.into())
        .init()
        .unwrap();
    let pinger = Box::leak(Box::new(
        Pinger::new(
            SocketAddr::from((args.host, 0)).into(),
            if args.count > 0 { args.count as u16 } else { u16::MAX },
            false,
            64,
            10,
            Duration::from_secs(10),
            Duration::from_secs(1),
        )
        .unwrap(),
    ));
    pinger.start().await;
}
