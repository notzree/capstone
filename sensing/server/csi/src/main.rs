//! `csi`: record, replay and inspect CSI sessions.

mod collect;
#[cfg(unix)]
mod star;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufWriter, Write as _};
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use csi_host::clock::{CLOCK_FIT_FILE, ClockFit, ClockFitter, ClockSummary, FitOptions};
use csi_host::inspect::{HealthEvent, InspectOptions, Inspector, Report};
use csi_host::manifest::{DerivedFrom, Manifest, Rotation};
use csi_host::pace::{Pace, Paced, SystemClock};
use csi_host::session::{Integrity, SegmentCheck, SessionReader, SessionWriter, WriterOptions};
use csi_host::synthetic::{Faults, Synthetic, SyntheticConfig};
use csi_host::{Record, RecordKind, StreamError};
use csi_protocol::{Message, frame};
use serde::Serialize;

#[derive(Parser)]
#[command(version, about = "Record, replay and inspect CSI sessions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run live serial/UDP ingest until Ctrl-C, SIGTERM or --seconds elapses
    Hub(HubArgs),
    /// Discover and start/stop a USB ESP32 star kit
    #[cfg(unix)]
    Star(star::StarArgs),
    /// Record a session from a source
    Record(RecordArgs),
    /// Replay a session, optionally re-recording it
    Replay(ReplayArgs),
    /// Report segment integrity, link rates, gaps, losses and health events
    Inspect(InspectArgs),
    /// Fit receiver clocks from shared probes and store clock-fit.jsonl in the session
    FitClocks(FitClocksArgs),
    /// Export a sealed session to Arrow/Parquet for Python
    Export(ExportArgs),
    /// Collect a trial with provenance, operator labels, quality and Python export
    Collect(collect::Args),
}

#[derive(Args)]
struct ExportArgs {
    session: PathBuf,
    /// New directory for data, provenance and quality summary
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_enum, default_value_t = ExportFormat::Arrow)]
    format: ExportFormat,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportFormat {
    Arrow,
    Parquet,
}

#[derive(Args)]
struct HubArgs {
    /// TOML source and runtime configuration
    #[arg(long)]
    config: PathBuf,
    /// New session directory
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    geometry: Option<PathBuf>,
    /// Stop and seal after this many seconds (otherwise wait for a signal)
    #[arg(long)]
    seconds: Option<f64>,
    /// Publish lossy live CSI batches as Arrow IPC on this new Unix socket
    #[cfg(unix)]
    #[arg(long)]
    arrow_socket: Option<PathBuf>,
    /// Live Arrow batch flush interval; 5..=1000 ms (smaller batches cost more overhead)
    #[cfg(unix)]
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u64).range(5..=1000))]
    arrow_flush_ms: u64,
}

#[derive(Args)]
struct ClockArgs {
    /// Clock-fit window: probes older than this leave the fit
    #[arg(long, default_value_t = 60.0)]
    clock_window_s: f64,
    /// How often receiver clocks are refitted
    #[arg(long, default_value_t = 10.0)]
    clock_step_s: f64,
}

impl ClockArgs {
    fn options(&self) -> Result<FitOptions> {
        let window_ns = self.clock_window_s * 1e9;
        let step_ns = self.clock_step_s * 1e9;
        if !(step_ns >= 1.0 && window_ns >= step_ns && window_ns < u64::MAX as f64) {
            bail!(
                "clock step must be at least 1 ns and no longer than a finite, representable window"
            );
        }
        Ok(FitOptions {
            window_ns: window_ns as u64,
            step_ns: step_ns as u64,
            ..FitOptions::default()
        })
    }
}

#[derive(Args)]
struct FitClocksArgs {
    session: PathBuf,
    #[command(flatten)]
    clock: ClockArgs,
    /// Replace an existing clock-fit.jsonl
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct RecordArgs {
    /// Session directory to create
    #[arg(long)]
    out: PathBuf,
    /// Start a new segment after this many MiB
    #[arg(long, default_value_t = 256)]
    segment_mib: u64,
    /// Start a new segment after this many seconds of recorded time
    #[arg(long, default_value_t = 600.0)]
    segment_seconds: f64,
    /// Installation geometry file (csi-geometry/1) to copy into the session
    #[arg(long)]
    geometry: Option<PathBuf>,
    #[command(subcommand)]
    source: Source,
}

#[derive(Subcommand)]
enum Source {
    /// Deterministic synthetic nodes; the CSI bytes are noise
    Synthetic(SyntheticArgs),
}

#[derive(Args)]
struct SyntheticArgs {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 10.0)]
    seconds: f64,
    #[arg(long, default_value_t = 1)]
    receivers: u16,
    #[arg(long, default_value_t = 1)]
    transmitters: u16,
    /// Probes per second from each transmitter
    #[arg(long, default_value_t = 100.0)]
    rate: f64,
    #[arg(long, default_value_t = 384)]
    csi_len: u16,
    /// Starting fault rates; the options below override individual ones
    #[arg(long, value_enum, default_value_t = FaultPreset::None)]
    faults: FaultPreset,
    #[arg(long)]
    air_loss: Option<f64>,
    #[arg(long)]
    pool_drop: Option<f64>,
    #[arg(long)]
    export_drop: Option<f64>,
    #[arg(long)]
    corrupt: Option<f64>,
    #[arg(long)]
    truncate: Option<f64>,
    #[arg(long)]
    duplicate: Option<f64>,
    #[arg(long)]
    reorder: Option<f64>,
    #[arg(long)]
    stream_error: Option<f64>,
    #[arg(long)]
    outages: Option<u32>,
    #[arg(long)]
    outage_ms: Option<f64>,
    #[arg(long)]
    reboots: Option<u32>,
    #[arg(long)]
    gain_step: Option<f64>,
    #[arg(long)]
    gain_invalid: Option<f64>,
}

#[derive(Clone, Copy, ValueEnum)]
enum FaultPreset {
    None,
    Typical,
}

impl SyntheticArgs {
    fn config(&self) -> SyntheticConfig {
        let base = match self.faults {
            FaultPreset::None => Faults::default(),
            FaultPreset::Typical => Faults::typical(),
        };
        SyntheticConfig {
            seed: self.seed,
            seconds: self.seconds,
            receivers: self.receivers,
            transmitters: self.transmitters,
            rate_hz: self.rate,
            csi_len: self.csi_len,
            faults: Faults {
                air_loss: self.air_loss.unwrap_or(base.air_loss),
                pool_drop: self.pool_drop.unwrap_or(base.pool_drop),
                export_drop: self.export_drop.unwrap_or(base.export_drop),
                corrupt: self.corrupt.unwrap_or(base.corrupt),
                truncate: self.truncate.unwrap_or(base.truncate),
                duplicate: self.duplicate.unwrap_or(base.duplicate),
                reorder: self.reorder.unwrap_or(base.reorder),
                stream_error: self.stream_error.unwrap_or(base.stream_error),
                outages: self.outages.unwrap_or(base.outages),
                outage_ms: self.outage_ms.unwrap_or(base.outage_ms),
                reboots: self.reboots.unwrap_or(base.reboots),
                gain_step: self.gain_step.unwrap_or(base.gain_step),
                gain_invalid: self.gain_invalid.unwrap_or(base.gain_invalid),
            },
            ..SyntheticConfig::default()
        }
    }
}

#[derive(Args)]
struct ReplayArgs {
    session: PathBuf,
    /// Playback speed relative to real time, or "max" for no pacing
    #[arg(long, default_value = "1", value_parser = parse_pace)]
    speed: Pace,
    /// Re-record the replay into this new session directory
    #[arg(long)]
    record: Option<PathBuf>,
    /// Geometry for the re-recorded session; defaults to the original's
    #[arg(long, requires = "record")]
    geometry: Option<PathBuf>,
    /// Print one line per record
    #[arg(long)]
    print: bool,
}

fn parse_pace(s: &str) -> Result<Pace, String> {
    if s == "max" {
        return Ok(Pace::Unpaced);
    }
    match s.parse::<f64>() {
        Ok(speed) if speed.is_finite() && speed > 0.0 => Ok(Pace::Speed(speed)),
        _ => Err("expected a positive number or \"max\"".into()),
    }
}

#[derive(Args)]
struct InspectArgs {
    session: PathBuf,
    /// Print the full report as JSON
    #[arg(long)]
    json: bool,
    /// Report link gaps and node silences longer than this
    #[arg(long, default_value_t = 100.0)]
    gap_ms: f64,
    /// Health events to list; all are counted
    #[arg(long, default_value_t = 20)]
    events: usize,
    #[command(flatten)]
    clock: ClockArgs,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Hub(args) => hub(args),
        Command::Record(args) => record(args),
        Command::Replay(args) => replay(args),
        Command::Inspect(args) => inspect(args),
        Command::FitClocks(args) => fit_clocks(args),
        Command::Collect(args) => collect::run(args),
        #[cfg(unix)]
        Command::Star(args) => star::run(args),
        Command::Export(args) => {
            let format = match args.format {
                ExportFormat::Arrow => csi_host::export::Format::Arrow,
                ExportFormat::Parquet => csi_host::export::Format::Parquet,
            };
            let summary = csi_host::export::session(&args.session, &args.out, format)?;
            println!(
                "exported {} CSI rows ({} decoded) to {}",
                summary.csi_rows,
                summary.decoded_rows,
                args.out.display()
            );
            Ok(())
        }
    }
}

fn hub(args: HubArgs) -> Result<()> {
    if args
        .seconds
        .is_some_and(|s| !s.is_finite() || s <= 0.0 || s > 31_536_000.0)
    {
        bail!("hub seconds must be positive, finite and no longer than one year");
    }
    let config = csi_host::live::config::HubConfig::load(&args.config)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(io::stderr)
        .init();
    tokio::runtime::Runtime::new()?.block_on(async {
        // Install signal handlers before opening transports or starting the recorder.
        #[cfg(unix)]
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        #[cfg(unix)]
        let publisher = args
            .arrow_socket
            .as_deref()
            .map(|path| {
                csi_host::delivery::Publisher::bind(path)?
                    .with_flush_interval_ms(args.arrow_flush_ms)
            })
            .transpose()?;
        let mut subscribers = Vec::new();
        #[cfg(unix)]
        let arrow_input = if publisher.is_some() {
            let (subscriber, rx, drops) = csi_host::live::subscription(8192)?;
            subscribers.push(subscriber);
            Some((rx, drops))
        } else {
            None
        };
        let hub = csi_host::live::start(
            config,
            &args.out,
            WriterOptions {
                geometry: args.geometry,
                ..WriterOptions::default()
            },
            subscribers,
        )
        .await?;
        #[cfg(unix)]
        let arrow_worker = if let (Some(publisher), Some((input, drops))) = (publisher, arrow_input)
        {
            let session = SessionReader::open(&args.out)?;
            Some(publisher.spawn(
                input,
                drops,
                session.manifest().clone(),
                session.geometry()?,
                args.out.clone(),
            )?)
        } else {
            None
        };
        println!("hub recording to {}", args.out.display());
        for (i, addr) in hub.addresses.iter().enumerate() {
            if let Some(addr) = addr {
                println!("source {i}: UDP listening on {addr}");
            }
        }
        let deadline = async {
            match args.seconds {
                Some(s) => tokio::time::sleep(std::time::Duration::from_secs_f64(s)).await,
                None => std::future::pending::<()>().await,
            }
        };
        let stop = async {
            #[cfg(unix)]
            tokio::select! {
                result = tokio::signal::ctrl_c() => result?,
                _ = term.recv() => {},
            }
            #[cfg(not(unix))]
            tokio::signal::ctrl_c().await?;
            Ok::<(), io::Error>(())
        };
        // The hub may fail before a signal/deadline; propagate that error immediately.
        let shutdown = async {
            tokio::select! { result = stop => { result?; }, _ = deadline => {} }
            Ok::<(), io::Error>(())
        };
        let mut completed = Box::pin(hub.wait_for_shutdown(shutdown));
        let status = (&mut completed).await;
        #[cfg(unix)]
        if let Some(worker) = arrow_worker {
            tokio::task::spawn_blocking(move || worker.join())
                .await?
                .map_err(|_| anyhow::anyhow!("Arrow publisher thread panicked"))??;
        }
        let status = status?;
        println!(
            "recorded {} records; session sealed; status: {}",
            status.records,
            args.out.join(csi_host::live::STATUS_FILE).display()
        );
        Ok(())
    })
}

fn fit_clocks(args: FitClocksArgs) -> Result<()> {
    let options = args.clock.options()?;
    let mut reader = SessionReader::open(&args.session)?;
    let mut fitter = ClockFitter::new(options);
    for record in reader.by_ref() {
        fitter.push(&record?);
    }
    let path = args.session.join(CLOCK_FIT_FILE);
    if path.exists() {
        if !args.force {
            bail!("{} exists; pass --force to replace it", path.display());
        }
        fs::remove_file(&path)?;
    }
    let fit = fitter.finish();
    fit.write(&args.session, reader.manifest().session_id)?;
    println!(
        "{} mappings for {} receiver clocks written to {}",
        fit.mappings.len(),
        fit.clocks.len(),
        path.display()
    );
    Ok(())
}

fn record(args: RecordArgs) -> Result<()> {
    if args.segment_mib == 0 || !(args.segment_seconds.is_finite() && args.segment_seconds > 0.0) {
        bail!("segment limits must be positive");
    }
    let options = WriterOptions {
        rotation: Rotation {
            max_bytes: args.segment_mib << 20,
            max_duration_ns: (args.segment_seconds * 1e9) as u64,
        },
        derived_from: None,
        geometry: args.geometry.clone(),
    };
    match args.source {
        Source::Synthetic(synthetic) => {
            let config = synthetic.config();
            let mut source = Synthetic::new(&config)?;
            let mut writer =
                SessionWriter::create(&args.out, Synthetic::sources(&config), options)?;
            let mut fitter = ClockFitter::new(FitOptions::default());
            let mut records = 0u64;
            for record in source.by_ref() {
                writer.write(&record)?;
                fitter.push(&record);
                records += 1;
            }
            let manifest = writer.finish()?;
            fitter.finish().write(&args.out, manifest.session_id)?;
            let truth = args.out.join("synthetic-truth.json");
            fs::write(&truth, serde_json::to_vec_pretty(&source.truth())?)?;
            println!(
                "recorded {records} records in {} segments to {}",
                manifest.segments.len(),
                args.out.display()
            );
            println!("injected faults and observable losses: {}", truth.display());
        }
    }
    Ok(())
}

fn replay(args: ReplayArgs) -> Result<()> {
    let reader = SessionReader::open(&args.session)?;
    let manifest = reader.manifest().clone();
    let mut writer = match &args.record {
        Some(out) => {
            let geometry = match (&args.geometry, &manifest.geometry) {
                (Some(path), _) => Some(path.clone()),
                (None, Some(original)) => {
                    reader.geometry()?;
                    Some(reader.dir().join(&original.file))
                }
                (None, None) => None,
            };
            Some(SessionWriter::create(
                out,
                manifest.sources.clone(),
                WriterOptions {
                    rotation: manifest.rotation,
                    derived_from: Some(DerivedFrom {
                        session_id: manifest.session_id,
                        how: "replay".into(),
                    }),
                    geometry,
                },
            )?)
        }
        None => None,
    };
    let mut out = BufWriter::new(io::stdout().lock());
    let mut paced = Paced::new(reader, args.speed, SystemClock::default());
    let mut fitter = ClockFitter::new(FitOptions::default());
    let mut records = 0u64;
    for record in paced.by_ref() {
        let record = record?;
        if args.print {
            writeln!(out, "{}", describe_record(&record))?;
        }
        if let Some(writer) = &mut writer {
            writer.write(&record)?;
            fitter.push(&record);
        }
        records += 1;
    }
    for check in paced.into_inner().checks() {
        if !matches!(check.integrity, Integrity::Verified) {
            writeln!(out, "segment {}: {:?}", check.file, check.integrity)?;
        }
    }
    match (writer, &args.record) {
        (Some(writer), Some(path)) => {
            let copy = writer.finish()?;
            fitter.finish().write(path, copy.session_id)?;
            writeln!(
                out,
                "re-recorded {records} records into {} ({} segments)",
                path.display(),
                copy.segments.len()
            )?;
        }
        _ => writeln!(out, "replayed {records} records")?,
    }
    Ok(())
}

fn describe_record(record: &Record) -> String {
    let body = match record.kind {
        RecordKind::Frame => match frame::decode(&record.payload) {
            Ok((header, message)) => {
                let what = match message {
                    Message::Csi(c) => {
                        let from = c
                            .probe
                            .map(|p| format!(" from {} probe {}", p.tx, p.seq))
                            .unwrap_or_default();
                        format!(
                            "csi capture {} len {} rssi {}{from}",
                            c.capture_seq,
                            c.data.len(),
                            c.rx.rssi
                        )
                    }
                    Message::Hello(h) => {
                        format!("hello {:?} rx {} tx {}", h.chip, h.roles.rx, h.roles.tx)
                    }
                    Message::Counters(c) => format!(
                        "counters captured {} pool_drops {} export_drops {}",
                        c.csi_captured, c.pool_drops, c.export_drops
                    ),
                    Message::TimeEcho(e) => format!("time_echo {}", e.probe_id),
                    Message::TimeProbe(p) => format!("time_probe {}", p.probe_id),
                };
                format!(
                    "{} epoch {:08x} seq {} {what}",
                    header.node, header.epoch, header.seq
                )
            }
            Err(e) => format!("undecodable frame of {} bytes: {e}", record.payload.len()),
        },
        RecordKind::StreamError => format!(
            "stream error {}",
            record
                .payload
                .first()
                .and_then(|&code| StreamError::from_code(code))
                .map_or("unknown", StreamError::name)
        ),
        RecordKind::WirelessBatch => match csi_protocol::wireless::Batch::decode(&record.payload) {
            Ok(batch) => format!(
                "wireless batch {}: {} original frames",
                batch.carrier.batch_seq, batch.count
            ),
            Err(error) => format!("invalid wireless batch: {error}"),
        },
        RecordKind::StarControl => format!(
            "star {:?}",
            csi_protocol::star::Packet::decode(&record.payload)
        ),
        RecordKind::Unknown(kind) => format!("unknown record kind {kind}"),
    };
    format!(
        "{:12.6} src {:<2} {body}",
        record.mono_ns as f64 / 1e9,
        record.source
    )
}

/// Events kept in memory before sorting by time; `--events` then chooses how many to show.
const EVENTS_KEPT: usize = 100_000;

#[derive(Serialize)]
struct Inspection<'a> {
    manifest: &'a Manifest,
    segments: &'a [SegmentCheck],
    geometry: GeometryCheck,
    stored_clock_fit: String,
    report: &'a Report,
}

#[derive(Serialize)]
struct GeometryCheck {
    /// "none", "ok" or "error".
    status: &'static str,
    room: Option<String>,
    revision: Option<String>,
    nodes: usize,
    error: Option<String>,
    /// Nodes in the session, receivers and transmitters, that the geometry does not place.
    missing: Vec<String>,
    /// Nodes the geometry places that never appeared in the session.
    unobserved: Vec<String>,
}

fn check_geometry(reader: &SessionReader, report: &Report) -> GeometryCheck {
    let observed: BTreeSet<String> = report
        .nodes
        .iter()
        .flat_map(|n| {
            let txs = n
                .epochs
                .iter()
                .flat_map(|e| e.links.iter().map(|l| l.tx.clone()));
            std::iter::once(n.node.clone()).chain(txs)
        })
        .collect();
    let mut check = GeometryCheck {
        status: "none",
        room: None,
        revision: None,
        nodes: 0,
        error: None,
        missing: observed.iter().cloned().collect(),
        unobserved: Vec::new(),
    };
    match reader.geometry() {
        Ok(None) => {}
        Ok(Some(geometry)) => {
            let placed: BTreeSet<String> = geometry
                .node_ids()
                .iter()
                .map(|id| id.to_string())
                .collect();
            check.status = "ok";
            check.nodes = placed.len();
            check.missing = observed.difference(&placed).cloned().collect();
            check.unobserved = placed.difference(&observed).cloned().collect();
            check.room = Some(geometry.room);
            check.revision = Some(geometry.revision);
        }
        Err(e) => {
            check.status = "error";
            check.error = Some(e.to_string());
        }
    }
    check
}

fn inspect(args: InspectArgs) -> Result<()> {
    if !(args.gap_ms.is_finite() && args.gap_ms > 0.0) {
        bail!("--gap-ms must be positive");
    }
    let mut reader = SessionReader::open(&args.session)?;
    let mut inspector = Inspector::new(InspectOptions {
        gap_threshold_ns: (args.gap_ms * 1e6) as u64,
        max_events: EVENTS_KEPT.max(args.events),
        clock: args.clock.options()?,
    });
    for record in reader.by_ref() {
        inspector.push(&record?);
    }
    let mut report = inspector.finish();
    let shown = args.events.min(report.events.len());
    report.events_omitted += (report.events.len() - shown) as u64;
    report.events.truncate(shown);
    let stored_clock_fit = match ClockFit::read(reader.dir(), reader.manifest().session_id) {
        Ok(Some(fit)) => format!("{CLOCK_FIT_FILE}: {} mappings", fit.mappings.len()),
        Ok(None) => format!("no {CLOCK_FIT_FILE} (csi fit-clocks writes one)"),
        Err(e) => format!("WARNING: {e}"),
    };
    let inspection = Inspection {
        manifest: reader.manifest(),
        segments: reader.checks(),
        geometry: check_geometry(&reader, &report),
        stored_clock_fit,
        report: &report,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&inspection)?);
    } else {
        print!("{}", render(&inspection));
    }
    Ok(())
}

fn render(inspection: &Inspection<'_>) -> String {
    let Inspection {
        manifest,
        segments,
        geometry,
        stored_clock_fit,
        report,
    } = inspection;
    let mut s = String::new();
    let status = if manifest.complete {
        "complete"
    } else {
        "INCOMPLETE"
    };
    let _ = writeln!(
        s,
        "session {} ({status}), {}, protocol v{}",
        manifest.session_id, manifest.recorder, manifest.protocol_version
    );
    if let Some(origin) = &manifest.derived_from {
        let _ = writeln!(s, "derived from {} by {}", origin.session_id, origin.how);
    }
    let mut integrity: Vec<String> = Vec::new();
    for check in *segments {
        match &check.integrity {
            Integrity::Verified => {}
            other => integrity.push(format!("{} {other:?}", check.file)),
        }
    }
    let verified = segments.len() - integrity.len();
    let _ = writeln!(s, "segments: {verified} of {} verified", segments.len());
    for problem in &integrity {
        let _ = writeln!(s, "  {problem}");
    }
    match (
        geometry.status,
        &geometry.room,
        &geometry.revision,
        &geometry.error,
    ) {
        ("ok", Some(room), Some(revision), _) => {
            let _ = writeln!(
                s,
                "geometry: room {room}, revision {revision}, {} nodes",
                geometry.nodes
            );
            if !geometry.missing.is_empty() {
                let _ = writeln!(
                    s,
                    "  WARNING: {} nodes in the session are not in the geometry: {}",
                    geometry.missing.len(),
                    geometry.missing.join(", ")
                );
            }
            if !geometry.unobserved.is_empty() {
                let _ = writeln!(
                    s,
                    "  placed but not observed: {}",
                    geometry.unobserved.join(", ")
                );
            }
        }
        (_, _, _, Some(error)) => {
            let _ = writeln!(s, "geometry: WARNING: {error}");
        }
        _ => {
            let _ = writeln!(s, "geometry: none (record with --geometry to attach one)");
        }
    }
    let _ = writeln!(s, "{} records over {:.3} s", report.records, report.span_s);

    for source in &report.sources {
        let name = manifest
            .sources
            .get(usize::from(source.index))
            .map_or("?", |info| info.name.as_str());
        let versions: Vec<String> = source
            .versions
            .iter()
            .map(|(v, n)| format!("v{v} {n}"))
            .collect();
        let _ = write!(
            s,
            "\nsource {} {name}: {} records, {} frames ({})",
            source.index,
            source.records,
            source.frames,
            versions.join(", ")
        );
        for (label, errors) in [
            ("decode errors", &source.decode_errors),
            ("stream errors", &source.stream_errors),
        ] {
            if !errors.is_empty() {
                let list: Vec<String> = errors.iter().map(|(k, v)| format!("{k} {v}")).collect();
                let _ = write!(s, "; {label}: {}", list.join(", "));
            }
        }
        let _ = writeln!(s);
    }

    for node in &report.nodes {
        let _ = writeln!(s, "\nnode {}", node.node);
        for e in &node.epochs {
            let kinds: Vec<String> = e.kinds.iter().map(|(k, v)| format!("{k} {v}")).collect();
            let _ = writeln!(
                s,
                "  epoch {:08x}  {:.3}-{:.3} s  {} frames ({})",
                e.epoch,
                e.first_s,
                e.last_s,
                e.frames,
                kinds.join(", ")
            );
            let _ = writeln!(
                s,
                "    frames missing {}, duplicate {}, late {}, stale {}; captures missing {}; clock backwards {}",
                e.frames_missing,
                e.frames_duplicate,
                e.frames_late,
                e.frames_stale,
                e.captures_missing,
                e.clock_backwards
            );
            if let Some(c) = &e.counters {
                let _ = writeln!(
                    s,
                    "    node counters: callbacks {}, filtered {}, captured {}, pool drops {}, export drops {}, tx {}/{}",
                    c.csi_callbacks,
                    c.csi_filtered,
                    c.csi_captured,
                    c.pool_drops,
                    c.export_drops,
                    c.tx_accepted,
                    c.tx_requested
                );
            }
            if let Some(h) = &e.hello {
                let _ = writeln!(
                    s,
                    "    hello: {} tx {} rx {}, firmware {} ({}), channel {}/{}, {}, pool {}",
                    h.chip,
                    h.tx,
                    h.rx,
                    h.firmware_version,
                    h.firmware_commit,
                    h.channel,
                    h.secondary_channel,
                    h.csi_config,
                    h.pool_capacity
                );
            }
            if e.time_echoes > 0 {
                let rtt = e
                    .min_rtt_us
                    .map_or("-".into(), |us| format!("{:.3} ms", us / 1e3));
                let _ = writeln!(s, "    time echoes {}, min rtt {rtt}", e.time_echoes);
            }
            for link in &e.links {
                let rate = link.rate_hz.map_or("-".into(), |r| format!("{r:.1} Hz"));
                let _ = writeln!(
                    s,
                    "    link from {}: {} records, {rate}, probes missing {}, duplicate {}, late {}",
                    link.tx,
                    link.records,
                    link.probes_missing,
                    link.probes_duplicate,
                    link.probes_late
                );
                let steps = link
                    .gain_steps_per_minute
                    .map_or("-".into(), |r| format!("{r:.2}/min"));
                let _ = writeln!(
                    s,
                    "      gain valid {:.1}%, {} gain steps ({steps})",
                    link.gain_valid_fraction * 100.0,
                    link.gain_steps
                );
                if let Some(p) = &link.interval_us {
                    let ms = |us: u64| us as f64 / 1e3;
                    let _ = writeln!(
                        s,
                        "      interval p50 {:.3} ms, p95 {:.3} ms, p99 {:.3} ms, max {:.3} ms; {} gaps over {} ms",
                        ms(p.p50),
                        ms(p.p95),
                        ms(p.p99),
                        ms(p.max),
                        link.gaps_over_threshold,
                        link.gap_threshold_ms
                    );
                }
                for layout in &link.layouts {
                    let l = &layout.layout;
                    let _ = writeln!(
                        s,
                        "      layout sig_mode {} cwb {} secondary {} stbc {} len {} first_word_invalid {}: {} records",
                        l.sig_mode,
                        l.cwb,
                        l.secondary_channel,
                        l.stbc,
                        l.len,
                        l.first_word_invalid,
                        layout.records
                    );
                }
            }
            if e.unattributed_csi > 0 {
                let _ = writeln!(s, "    csi without a probe: {}", e.unattributed_csi);
            }
        }
    }

    render_clock(&mut s, &report.clock, stored_clock_fit);

    let total: u64 = report.event_counts.values().sum();
    let counts: Vec<String> = report
        .event_counts
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    let _ = writeln!(s, "\nevents: {total} ({})", counts.join(", "));
    for event in &report.events {
        let _ = writeln!(
            s,
            "  {:10.3} s  {}",
            event.mono_ns() as f64 / 1e9,
            describe_event(event)
        );
    }
    if report.events_omitted > 0 {
        let _ = writeln!(
            s,
            "  ... {} more (raise --events to list them)",
            report.events_omitted
        );
    }
    s
}

fn render_clock(s: &mut String, clock: &ClockSummary, stored: &str) {
    let reference = clock.reference.as_deref().unwrap_or("none");
    let _ = writeln!(
        s,
        "\nclock fit (window {} s, step {} s): reference {reference}, {} handovers, {} timeline restarts",
        clock.window_s, clock.step_s, clock.handovers, clock.timeline_restarts
    );
    for receiver in &clock.receivers {
        let Some(m) = &receiver.latest else {
            let _ = writeln!(
                s,
                "  {}: not mapped (shares too few probes)",
                receiver.clock
            );
            continue;
        };
        let Some(residual) = &m.residual_us else {
            let _ = writeln!(
                s,
                "  {}: reference ({} fits)",
                receiver.clock, receiver.fits
            );
            continue;
        };
        let [low, high] = receiver.drift_ppm_range.unwrap_or([m.drift_ppm; 2]);
        let via = m.parent.map_or("-".into(), |p| p.to_string());
        let _ = writeln!(
            s,
            "  {}: offset {:+.3} µs, drift {:+.4} ppm ({low:+.4}..{high:+.4} over {} fits)",
            receiver.clock, m.offset_us, m.drift_ppm, receiver.fits
        );
        let _ = writeln!(
            s,
            "    residual p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} µs, {} pairs, {} outliers, via {via} ({} hops)",
            residual.p50, residual.p95, residual.p99, residual.max, m.pairs, m.outliers, m.hops
        );
    }
    if clock.unwrap_rejects + clock.radio_restarts > 0 {
        let _ = writeln!(
            s,
            "  WARNING: {} records could not be unwrapped; {} radio timer restarts",
            clock.unwrap_rejects, clock.radio_restarts
        );
    }
    let _ = writeln!(s, "  stored: {stored}");
}

fn describe_event(event: &HealthEvent) -> String {
    match event {
        HealthEvent::NodeEpoch {
            node,
            epoch,
            previous: Some(previous),
            ..
        } => format!("reboot {node}: epoch {previous:08x} -> {epoch:08x}"),
        HealthEvent::NodeEpoch { node, epoch, .. } => format!("node {node} epoch {epoch:08x}"),
        HealthEvent::Silence {
            node, silent_ms, ..
        } => format!("silence {node}: {silent_ms:.1} ms"),
        HealthEvent::FramesMissing {
            node,
            epoch,
            first_seq,
            count,
            ..
        } => format!("frames missing {node} epoch {epoch:08x}: {count} from seq {first_seq}"),
        HealthEvent::DuplicateFrame { node, seq, .. } => {
            format!("duplicate frame {node} seq {seq}")
        }
        HealthEvent::LateFrame { node, seq, .. } => format!("late frame {node} seq {seq}"),
        HealthEvent::ClockBackwards { node, epoch, .. } => {
            format!("clock backwards {node} epoch {epoch:08x}")
        }
        HealthEvent::LayoutChange {
            node, tx, layout, ..
        } => format!("layout change {node} from {tx}: {layout:?}"),
        HealthEvent::DecodeError { source, error, .. } => {
            format!("decode error on source {source}: {error}")
        }
        HealthEvent::StreamError { source, error, .. } => {
            format!("stream error on source {source}: {error}")
        }
    }
}
