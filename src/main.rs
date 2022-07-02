use clap::{ArgAction, Parser, Subcommand, ValueEnum};

use dns_lookup::lookup_host;
use futures::{stream, StreamExt};
use log::{error, trace};
use pinger::Pinger;
use std::io;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::{select, signal};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Style};

use tui::widgets::{Axis, Block, Borders, Chart};
use tui::Terminal;

mod pinger;
mod plot_data;

#[derive(Parser, Debug)]
#[clap(version)]
struct Cli {
    #[clap(short, long, help = "add more v to get more detailed output", action = ArgAction::Count)]
    verbosity: u8,
    #[clap(short, long, help = "disable log output", action = ArgAction::SetTrue)]
    quiet: bool,
    #[clap(long, value_enum, default_value = "off")]
    timestamp: Timestamp,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Trace {
        #[clap(help = "host to ping")]
        hosts: Vec<String>,
        #[clap(short, long, default_value = "32")]
        size: u16,
        #[clap(short, long, help = "Draw topology graph", action = ArgAction::SetTrue)]
        graph: bool,
        #[clap(long, help = "Timeout for each ping", default_value = "5s")]
        timeout: humantime::Duration,
    },
    Ping {
        #[clap(help = "host to ping")]
        host: String,
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
        #[clap(short, long, help = "Draw latency graph", action = ArgAction::SetTrue)]
        graph: bool,
    },
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
    let graph = match args.command {
        Commands::Trace { .. } => false,
        Commands::Ping { graph, .. } => graph,
    };
    stderrlog::new()
        .module(module_path!())
        .quiet(args.quiet || graph)
        .verbosity((args.verbosity + 2) as usize)
        .timestamp(args.timestamp.clone().into())
        .init()
        .unwrap();
    trace!("args = {args:?}");

    match args.command {
        Commands::Trace {
            hosts,
            size,
            graph,
            timeout,
        } => {
            let results = stream::iter(hosts.into_iter().map(|h| {
                lookup_host(&h)
                    .unwrap()
                    .into_iter()
                    .find_map(|x| match x {
                        std::net::IpAddr::V4(x) => Some(x),
                        _ => None,
                    })
                    .unwrap()
            }))
            .then(|host| async move {
                let (tx, _) = mpsc::channel(1);
                let pinger = Box::leak(Box::new(
                    Pinger::new(
                        SocketAddr::from((host, 0)).into(),
                        0,
                        false,
                        size,
                        128,
                        timeout.into(),
                        Duration::from_secs(1),
                        false,
                        tx,
                        graph,
                    )
                    .unwrap(),
                ));
                pinger.traceroute().await.unwrap()
            })
            .collect::<Vec<_>>()
            .await;
            trace!("{:?}", results);
            if graph {
                let max_length = results.iter().map(|x| x.len()).max().unwrap_or(0);
                let mut same_length = 0;
                for i in 0..max_length {
                    if results.iter().all(|r| {
                        r.len() > i
                            && r[i].is_some()
                            && results[0][i].is_some()
                            && r[i].unwrap().0 == results[0][i].unwrap().0
                    }) {
                        same_length += 1;
                    } else {
                        break;
                    }
                }
                if same_length == 0 {
                    println!("* localhost");
                }
                for i in 0..same_length {
                    println!("* {}", results[0][i].unwrap().0);
                    if i != same_length - 1 {
                        println!("|");
                    }
                }
                print!("| ");
                for _ in 1..results.len() {
                    print!("\\ ");
                }
                println!();
                for i in same_length..max_length {
                    for j in 0..results.len() {
                        if results[j].len() <= i {
                            continue;
                        }
                        // if i != same_length {
                        //     for k in 0..results.len() {
                        //         if results[k].len() > i {
                        //             print!("| ");
                        //         } else {
                        //             print!("  ");
                        //         }
                        //     }
                        //     println!();
                        // }

                        for k in 0..results.len() {
                            if j == k {
                                print!("* ");
                            } else if results[k].len() > i
                                && (if results[k].len() == i + 1 {
                                    k > j
                                } else {
                                    true
                                })
                            {
                                print!("| ");
                            } else {
                                print!("  ");
                            }
                        }
                        println!(
                            "  {}",
                            match results[j][i] {
                                Some(x) => x.0.to_string(),
                                None => "*".to_string(),
                            }
                        );
                    }
                }
            }
        }
        Commands::Ping {
            host: _host,
            count,
            broadcast,
            interval,
            ttl,
            size,
            route,
            timeout,
            graph,
        } => {
            let (tx, mut rx) = mpsc::channel(10);
            let host: Vec<Ipv4Addr> = lookup_host(&_host)
                .unwrap()
                .into_iter()
                .filter_map(|x| match x {
                    std::net::IpAddr::V4(x) => Some(x),
                    _ => None,
                })
                .collect();
            if host.is_empty() {
                error!("{} is not a valid host", _host);
                return;
            }

            let mut data = plot_data::PlotData::new(
                _host.to_string(),
                150.0,
                Style::default().fg(Color::Gray),
                false,
            );

            let pinger = Box::leak(Box::new(
                Pinger::new(
                    SocketAddr::from((host[0], 0)).into(),
                    if count >= 0 { count as u16 } else { u16::MAX },
                    broadcast,
                    size,
                    ttl,
                    timeout.into(),
                    interval.into(),
                    route,
                    tx,
                    graph,
                )
                .unwrap(),
            ));
            let stdout = io::stdout();
            // execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend).unwrap();
            if graph {
                // enable_raw_mode().unwrap();

                terminal.clear().unwrap();
                tokio::spawn(async move {
                    while let Some(update) = rx.recv().await {
                        data.update(update);
                        // update
                        terminal
                            .draw(|f| {
                                // Split our
                                let chunks = Layout::default()
                                    .direction(Direction::Vertical)
                                    .vertical_margin(1)
                                    .horizontal_margin(0)
                                    .constraints(
                                        [Constraint::Length(1), Constraint::Percentage(100)]
                                            .as_mut_slice(),
                                    )
                                    .split(f.size());

                                let header_chunks = chunks[0].to_owned();
                                let chart_chunk = chunks[1].to_owned();

                                let header_layout = Layout::default()
                                    .direction(Direction::Horizontal)
                                    .constraints(
                                        [
                                            Constraint::Percentage(28),
                                            Constraint::Percentage(12),
                                            Constraint::Percentage(12),
                                            Constraint::Percentage(12),
                                            Constraint::Percentage(12),
                                            Constraint::Percentage(12),
                                            Constraint::Percentage(12),
                                        ]
                                        .as_ref(),
                                    )
                                    .split(header_chunks);

                                for (area, paragraph) in
                                    header_layout.into_iter().zip(data.header_stats())
                                {
                                    f.render_widget(paragraph, area);
                                }

                                let datasets = vec![(&data).dataset()];

                                let y_axis_bounds = data.y_axis_bounds();
                                let x_axis_bounds = data.x_axis_bounds();

                                let chart = Chart::new(datasets)
                                    .block(Block::default().borders(Borders::NONE))
                                    .x_axis(
                                        Axis::default()
                                            .style(Style::default().fg(Color::Gray))
                                            .bounds(x_axis_bounds)
                                            .labels(data.x_axis_labels(x_axis_bounds)),
                                    )
                                    .y_axis(
                                        Axis::default()
                                            .style(Style::default().fg(Color::Gray))
                                            .bounds(y_axis_bounds)
                                            .labels(data.y_axis_labels(y_axis_bounds)),
                                    );

                                f.render_widget(chart, chart_chunk);
                            })
                            .unwrap();
                    }
                });
            }
            select! {
                _ = signal::ctrl_c() => {},
                _ =  pinger.start() => {}
            }

            trace!("{:?}", pinger);
            let received = pinger
                .latencies
                .lock()
                .await
                .iter()
                .filter(|x| x.is_some())
                .count();
            let all = pinger.latencies.lock().await.len();
            println!(
                "sent {all} packages, received {received} packages, loss rate {:.1}%",
                (all - received) as f64 / all as f64 * 100.0
            );
            let average = pinger
                .latencies
                .lock()
                .await
                .iter()
                .flatten()
                .sum::<std::time::Duration>()
                .as_micros() as f64
                / received as f64
                / 1000.0;
            println!("Average latency: {average:.2} ms");
        }
    }
}
