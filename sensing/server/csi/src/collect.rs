//! Interactive collection keeps annotations on the source-arrival monotonic clock.
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use csi_host::live::config::HubConfig;
use csi_host::session::{SessionReader, WriterOptions, wall_now_ns};
use csi_host::trial::{Bundle, Labels, Stamp};
use tokio::sync::mpsc;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    config: PathBuf,
    /// Trial plan (csi-trial-plan/1); artifact paths are relative to this file
    #[arg(long)]
    plan: PathBuf,
    /// Survey matching the plan's room and geometry revision
    #[arg(long)]
    geometry: PathBuf,
    /// New bundle directory: raw/, provenance/, labels and export/
    #[arg(long)]
    out: PathBuf,
    /// Optional upper bound; stop/EOF/Ctrl-C can end collection earlier
    #[arg(long)]
    seconds: Option<f64>,
    /// Publish live CSI as Arrow IPC on this new Unix socket while collecting
    #[cfg(unix)]
    #[arg(long)]
    arrow_socket: Option<PathBuf>,
    #[cfg(unix)]
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u64).range(5..=1000))]
    arrow_flush_ms: u64,
}

const HELP: &str = "Commands: begin ACTIVITY [POSITION], end, note TEXT, help, stop.\nEnd each activity explicitly. EOF, Ctrl-C and stop close an open activity as truncated.";

fn command(labels: &mut Labels, text: &str, stamp: Stamp) -> Result<bool, csi_host::Error> {
    let text = text.trim();
    let words: Vec<_> = text.split_whitespace().collect();
    match words.as_slice() {
        [] => {}
        ["help"] => println!("{HELP}"),
        ["begin", activity] => {
            labels.begin(activity, None, stamp)?;
            println!("Activity: {activity}");
        }
        ["begin", activity, position] => {
            labels.begin(activity, Some(position), stamp)?;
            println!("Activity: {activity} at {position}");
        }
        ["end"] => {
            labels.end(stamp, "operator", false)?;
            println!("Activity ended");
        }
        ["stop"] => return Ok(true),
        ["note", ..] if words.len() >= 2 => {
            labels.note(text[4..].trim(), stamp)?;
            println!("Note recorded");
        }
        _ => return Err(csi_host::Error::Config("unknown command; type help".into())),
    }
    Ok(false)
}

pub fn run(args: Args) -> Result<()> {
    if args
        .seconds
        .is_some_and(|s| !s.is_finite() || s <= 0.0 || s > 31_536_000.0)
    {
        bail!("seconds must be positive, finite and no longer than one year");
    }
    let mut bundle = Bundle::prepare(&args.plan, &args.config, &args.geometry, &args.out)?;
    let config = HubConfig::load(&args.out.join("provenance/hub-config.toml"))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(io::stderr)
        .init();
    tokio::runtime::Runtime::new()?.block_on(async {
        #[cfg(unix)]
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        #[cfg(unix)]
        let publisher = args.arrow_socket.as_deref().map(|p| csi_host::delivery::Publisher::bind(p)?
            .with_flush_interval_ms(args.arrow_flush_ms)).transpose()?;
        let mut subscribers = Vec::new();
        #[cfg(unix)]
        let arrow_input = if publisher.is_some() {
            let (subscriber, input, drops) = csi_host::live::subscription(8192)?;
            subscribers.push(subscriber);
            Some((input, drops))
        } else { None };
        let hub = csi_host::live::start(config, &args.out.join("raw"), WriterOptions {
            geometry: Some(args.out.join("provenance/geometry.json")), ..Default::default()
        }, subscribers).await?;
        let clock = hub.clock();
        let reader = SessionReader::open(&args.out.join("raw"))?;
        #[cfg(unix)]
        let arrow_worker = if let (Some(publisher), Some((input, drops))) = (publisher, arrow_input) {
            Some(publisher.spawn(input, drops, reader.manifest().clone(), reader.geometry()?, args.out.join("raw"))?)
        } else { None };
        let mut labels = bundle.bind(reader.manifest())?;
        println!("Collecting trial {} into {}", bundle.plan.trial_id, args.out.display());
        for (i, addr) in hub.addresses.iter().enumerate() {
            if let Some(addr) = addr { println!("source {i}: UDP listening on {addr}"); }
        }
        println!("Activities: {}", bundle.plan.activities.join(", "));
        println!("{HELP}");
        // A detached ordinary stdin thread can be blocked in read_line while
        // SIGTERM/deadline still shuts down the hub. Tokio stdin would keep its
        // blocking-pool task alive during runtime destruction.
        let (tx, mut rx) = mpsc::channel(32);
        let input_clock = clock.clone();
        std::thread::Builder::new().name("csi-label-input".into()).spawn(move || {
            for line in io::stdin().lock().lines() {
                let stamp = Stamp { mono_ns: input_clock.mono_ns(), wall_ns: wall_now_ns() };
                if tx.blocking_send((stamp, line)).is_err() { return; }
            }
        })?;
        let deadline = async {
            if let Some(s) = args.seconds { tokio::time::sleep(Duration::from_secs_f64(s)).await; }
            else { std::future::pending::<()>().await; }
        };
        tokio::pin!(deadline);
        let mut stopped = None;
        let shutdown = async {
            let (stop_stamp, reason) = loop {
                #[cfg(unix)]
                let terminate = term.recv();
                #[cfg(not(unix))]
                let terminate = std::future::pending::<()>();
                tokio::select! {
                    input = rx.recv() => {
                        let Some((stamp, line)) = input else { break (Stamp { mono_ns: clock.mono_ns(), wall_ns: wall_now_ns() }, "stdin_eof"); };
                        let line = line?;
                        match command(&mut labels, &line, stamp) {
                            Ok(true) => break (stamp, "operator_stop"),
                            Ok(false) => {},
                            Err(csi_host::Error::Config(error)) => eprintln!("Label rejected: {error}"),
                            Err(error) => return Err(io::Error::other(error.to_string())),
                        }
                    },
                    signal = tokio::signal::ctrl_c() => { signal?; break (Stamp { mono_ns: clock.mono_ns(), wall_ns: wall_now_ns() }, "ctrl_c"); },
                    _ = terminate => break (Stamp { mono_ns: clock.mono_ns(), wall_ns: wall_now_ns() }, "sigterm"),
                    _ = &mut deadline => break (Stamp { mono_ns: clock.mono_ns(), wall_ns: wall_now_ns() }, "deadline"),
                }
            };
            // Persist queued annotations already read before the stop decision.
            while let Ok((stamp, line)) = rx.try_recv() {
                if stamp.mono_ns > stop_stamp.mono_ns { break; }
                if let Ok(line) = line {
                    match command(&mut labels, &line, stamp) {
                        Ok(true) => break,
                        Ok(false) => {},
                        Err(csi_host::Error::Config(error)) => eprintln!("Label rejected: {error}"),
                        Err(error) => return Err(io::Error::other(error.to_string())),
                    }
                }
            }
            stopped = Some((stop_stamp, reason));
            Ok(())
        };
        let status = hub.wait_for_shutdown(shutdown).await;
        #[cfg(unix)]
        if let Some(worker) = arrow_worker {
            tokio::task::spawn_blocking(move || worker.join()).await?
                .map_err(|_| anyhow::anyhow!("Arrow publisher thread panicked"))??;
        }
        let (stamp, reason) = stopped.unwrap_or((Stamp { mono_ns: clock.mono_ns(), wall_ns: wall_now_ns() }, "recorder_error"));
        labels.finish(stamp, reason)?;
        drop(labels);
        status?;
        let summary = csi_host::export::session(&args.out.join("raw"), &args.out.join("export"), csi_host::export::Format::Arrow)?;
        bundle.finish(stamp.mono_ns, reason)?;
        println!("Trial sealed: {} CSI rows, {} decoded; labels and quality: {}", summary.csi_rows, summary.decoded_rows, args.out.display());
        Ok(())
    })
}
