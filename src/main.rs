use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use crossterm::event::{KeyEvent, KeyModifiers};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dns_lookup::lookup_host;
use log::trace;
use pinger::Pinger;
use std::io;
use std::iter;
use std::net::IpAddr;
use std::net::{Ipv4Addr, SocketAddr};
use std::ops::Add;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::{select, signal};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::text::Span;
use tui::widgets::{Axis, Block, Borders, Chart, Dataset};
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
    #[clap(short, long, help = "Draw latency graph", action = ArgAction::SetTrue)]
    graph: bool,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Trace {},
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
        .quiet(args.quiet || args.graph)
        .verbosity((args.verbosity + 2) as usize)
        .timestamp(args.timestamp.clone().into())
        .init()
        .unwrap();
    trace!("args = {args:?}");
    let (tx, mut rx) = mpsc::channel(10);
    let mut data = plot_data::PlotData::new(
        args.host.to_string(),
        150.0,
        Style::default().fg(Color::Gray),
        false,
    );

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
            tx,
            args.graph,
        )
        .unwrap(),
    ));
    match args.command {
        Some(Commands::Trace {}) => {
            pinger.traceroute().await.unwrap();
        }
        None => {
            let mut stdout = io::stdout();
            // execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend).unwrap();
            if args.graph {
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
                                        [
                                            Constraint::Length(1),
                                            Constraint::Percentage(10),
                                        ].as_mut_slice()
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

                                let datasets = vec![(&data).into()];

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

            // disable_raw_mode().unwrap();
            // execute!(
            //     terminal.backend_mut(),
            //     LeaveAlternateScreen,
            //     DisableMouseCapture
            // ).unwrap();
            // terminal.show_cursor().unwrap();
        }
    }
}
