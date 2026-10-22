use std::fs;
use std::path::Path;
use std::time::Duration;

use csi_host::manifest::{DerivedFrom, Manifest, Rotation};
use csi_host::pace::{Clock, Pace, Paced};
use csi_host::session::{SessionReader, SessionWriter, WriterOptions};
use csi_host::synthetic::{Faults, Synthetic, SyntheticConfig};
use csi_host::{Record, RecordKind};

fn config() -> SyntheticConfig {
    SyntheticConfig {
        seed: 7,
        seconds: 4.0,
        receivers: 2,
        transmitters: 2,
        rate_hz: 200.0,
        faults: Faults::typical(),
        ..SyntheticConfig::default()
    }
}

fn record_synthetic(dir: &Path, config: &SyntheticConfig, rotation: Rotation) -> Manifest {
    let options = WriterOptions {
        rotation,
        ..WriterOptions::default()
    };
    let mut writer = SessionWriter::create(dir, Synthetic::sources(config), options).unwrap();
    for record in Synthetic::new(config).unwrap() {
        writer.write(&record).unwrap();
    }
    writer.finish().unwrap()
}

#[test]
fn synthetic_output_is_determined_by_its_config() {
    let a: Vec<Record> = Synthetic::new(&config()).unwrap().collect();
    let b: Vec<Record> = Synthetic::new(&config()).unwrap().collect();
    assert!(a.len() > 2000);
    assert_eq!(a, b);
    assert!(a.windows(2).all(|w| w[0].mono_ns <= w[1].mono_ns));

    let other = SyntheticConfig {
        seed: 8,
        ..config()
    };
    let c: Vec<Record> = Synthetic::new(&other).unwrap().collect();
    assert_ne!(a, c);
}

#[test]
fn rerecording_a_replay_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let original_dir = tmp.path().join("original");
    let rotation = Rotation {
        max_bytes: 200_000,
        max_duration_ns: 1_500_000_000,
    };
    let original = record_synthetic(&original_dir, &config(), rotation);
    assert!(original.segments.len() > 2);

    let reader = SessionReader::open(&original_dir).unwrap();
    let options = WriterOptions {
        rotation: original.rotation,
        derived_from: Some(DerivedFrom {
            session_id: original.session_id,
            how: "replay".into(),
        }),
        ..WriterOptions::default()
    };
    let copy_dir = tmp.path().join("copy");
    let mut writer = SessionWriter::create(&copy_dir, original.sources.clone(), options).unwrap();
    for record in Paced::new(
        reader,
        Pace::Unpaced,
        csi_host::pace::SystemClock::default(),
    ) {
        writer.write(&record.unwrap()).unwrap();
    }
    let copy = writer.finish().unwrap();

    assert_eq!(copy.segments, original.segments);
    for segment in &original.segments {
        assert_eq!(
            fs::read(original_dir.join(&segment.file)).unwrap(),
            fs::read(copy_dir.join(&segment.file)).unwrap()
        );
    }
    assert_eq!(copy.sources, original.sources);
    assert_eq!(copy.derived_from.unwrap().session_id, original.session_id);
    assert_ne!(copy.session_id, original.session_id);
}

#[derive(Default)]
struct FakeClock {
    now: Duration,
    deadlines: Vec<Duration>,
}

impl Clock for FakeClock {
    fn now(&mut self) -> Duration {
        self.now
    }

    fn sleep_until(&mut self, deadline: Duration) {
        self.deadlines.push(deadline);
        self.now = self.now.max(deadline);
    }
}

fn at(mono_ns: u64) -> Result<Record, ()> {
    Ok(Record {
        mono_ns,
        wall_ns: 0,
        source: 0,
        kind: RecordKind::Frame,
        payload: Vec::new(),
    })
}

#[test]
fn pacing_follows_recorded_spacing_divided_by_speed() {
    let mut clock = FakeClock {
        now: Duration::from_secs(5),
        ..FakeClock::default()
    };
    let input = vec![at(100_000_000), at(150_000_000), at(1_150_000_000)];
    let output: Vec<_> =
        Paced::new(input.clone().into_iter(), Pace::Speed(2.0), &mut clock).collect();
    assert_eq!(output, input);
    let ms = |n| Duration::from_secs(5) + Duration::from_millis(n);
    assert_eq!(clock.deadlines, [ms(0), ms(25), ms(525)]);
}

#[test]
fn unpaced_replay_never_waits() {
    let mut clock = FakeClock::default();
    let input = vec![at(0), at(1_000_000_000)];
    assert_eq!(
        Paced::new(input.into_iter(), Pace::Unpaced, &mut clock).count(),
        2
    );
    assert!(clock.deadlines.is_empty());
}
