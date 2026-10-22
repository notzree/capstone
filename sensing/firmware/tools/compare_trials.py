#!/usr/bin/env python3
"""Read-only bench comparison; never opens devices or changes recordings.

Registry: {"trials": [{"condition": "pair-10", "mode": "pair", "target_hz": 10,
"repeat": 1, "audit": "pair-10-r1-audit.json"}], "notes": []}.
Audit paths are relative to the registry. JSON, Markdown, and standalone SVG are
written to --out-dir. Replicates remain separate; aggregates are descriptive.
"""
import argparse
from collections import defaultdict
import html
import json
from pathlib import Path
import statistics


def describe(values):
    values = [v for v in values if v is not None]
    return None if not values else {"n": len(values), "mean": statistics.mean(values),
                                    "min": min(values), "max": max(values)}


def sum_field(rows, field):
    values = [r.get(field) for r in rows]
    return None if not values or any(v is None for v in values) else sum(values)


def summarize_trial(trial, base):
    path = base / trial["audit"]
    audit = json.loads(path.read_text())
    health = audit["health"]
    epoch_rows = [e for n in health["nodes"] for e in n["epochs"]]
    counters = [n["counter_delta"] or {} for n in audit["nodes"]]
    sources = health["sources"]
    errors = {
        "stream_or_decode": sum(audit["stream_or_decode_errors"].values()),
        "stream_or_decode_first_second": audit.get("stream_or_decode_errors_first_second"),
        "stream_or_decode_after_first_second": audit.get("stream_or_decode_errors_after_first_second"),
        "frames_missing": sum_field(epoch_rows, "frames_missing"),
        "captures_missing": sum_field(epoch_rows, "captures_missing"),
        "pool_drops": sum_field(counters, "pool_drops"),
        "export_drops": sum_field(counters, "export_drops"),
        "frame_decode": sum(sum(s["decode_errors"].values()) for s in sources),
        "stream": sum(sum(s["stream_errors"].values()) for s in sources),
        "self_captures": audit["self_captures"],
        "probe_source_mac_mismatches": audit["probe_source_mac_mismatches"],
    }
    links = []
    for link in audit["links"]:
        rx_health = [e for n in health["nodes"] if n["node"] == link["rx"]
                     for e in n["epochs"] if e["epoch"] == link["rx_epoch"]]
        unique, missing = link["probes_unique"], link["probes_missing"]
        tx_reports = [n for n in audit["nodes"] if n["node"] == link["tx"]
                      and n["epoch"] == link["tx_epoch"]]
        tx_delta = tx_reports[0]["counter_delta"] if len(tx_reports) == 1 else None
        rx_reports = [n for n in audit["nodes"] if n["node"] == link["rx"]
                      and n["epoch"] == link["rx_epoch"]]
        rx_delta = (rx_reports[0]["counter_delta"] or {}) if len(rx_reports) == 1 else {}
        counter_s = ((tx_delta or {}).get("node_time_us") or 0) / 1e6
        tx_rate = lambda field: (tx_delta[field] / counter_s if counter_s and
                                tx_delta.get(field) is not None else None)
        links.append({
            "direction": f'{link["tx"]} -> {link["rx"]}',
            "rx_epoch": link["rx_epoch"], "tx_epoch": link["tx_epoch"],
            "schedule": link["schedule"], "records": link["records"],
            "rate_hz": link["rate_hz"], "probes_unique": unique,
            "transmitter_requested_hz": tx_rate("tx_requested"),
            "transmitter_accepted_hz": tx_rate("tx_accepted"),
            "transmitter_counter_span_s": counter_s or None,
            "probes_missing": missing,
            "interior_missing_fraction": missing / (unique + missing) if unique + missing else None,
            "probes_duplicate": link["probes_duplicate"],
            "probes_late": link["probes_late"], "probes_stale": link["probes_stale"],
            "interarrival_node_us": link.get("interarrival_node_us"),
            "rssi_dbm": link.get("rssi_dbm"), "noise_floor_dbm": link.get("noise_floor_dbm"),
            "node_timestamp_regressions": link.get("node_timestamp_regressions"),
            "receiver_frames_missing": sum_field(rx_health, "frames_missing"),
            "receiver_captures_missing": sum_field(rx_health, "captures_missing"),
            "receiver_pool_drops": rx_delta.get("pool_drops"),
            "receiver_export_drops": rx_delta.get("export_drops"),
        })
    expected = trial.get("expected_directions", [])
    observed = [l["direction"] for l in links]
    return {**trial, "audit": str(path.resolve()), "span_s": health["span_s"],
            "links": links, "errors": errors,
            "missing_expected_directions": [d for d in expected if d not in observed],
            "sealed_integrity_verified": bool(audit["segment_checks"]) and all(
                s["sealed"] and s["integrity"]["status"] == "verified" for s in audit["segment_checks"]),
            "nodes": audit["nodes"],
            "error_samples_first_32": audit.get("stream_or_decode_error_samples_first_32", [])}


def summarize(registry, base):
    trials = [summarize_trial(t, base) for t in registry["trials"]]
    grouped = defaultdict(list)
    for t in trials:
        # Separate schedules/epochs remain visible; do not silently average a reset.
        for link in t["links"]:
            grouped[(t["condition"], link["direction"])].append((t, link))
    aggregates = []
    for (condition, direction), rows in sorted(grouped.items()):
        intervals = [l.get("interarrival_node_us") or {} for _, l in rows]
        unique = sum(l["probes_unique"] for _, l in rows)
        missing = sum(l["probes_missing"] for _, l in rows)
        aggregates.append({
            "condition": condition, "direction": direction,
            "mode": rows[0][0]["mode"], "target_hz": rows[0][0]["target_hz"],
            "trial_count": len({t["repeat"] for t, _ in rows}), "link_runs": len(rows),
            "rate_hz": describe([l["rate_hz"] for _, l in rows]),
            "transmitter_requested_hz": describe([l["transmitter_requested_hz"] for _, l in rows]),
            "transmitter_accepted_hz": describe([l["transmitter_accepted_hz"] for _, l in rows]),
            "interior_missing_fraction": describe([l["interior_missing_fraction"] for _, l in rows]),
            "pooled_interior_missing_fraction": missing / (unique + missing) if unique + missing else None,
            "probes_unique": unique, "probes_missing": missing,
            "interarrival_p95_us": describe([i.get("p95") for i in intervals]),
            "interarrival_p99_us": describe([i.get("p99") for i in intervals]),
            "interarrival_max_us": describe([i.get("max") for i in intervals]),
            "interarrival_stddev_us": describe([i.get("stddev") for i in intervals]),
            "rssi_mean_dbm": describe([(l.get("rssi_dbm") or {}).get("mean") for _, l in rows]),
        })
    return {"trials": trials, "aggregates": aggregates, "notes": registry.get("notes", []) + [
        "Descriptive pilot only; repeated windows on the same desk are not independent room/person/day trials.",
        "Compare modes at the same per-node offered rate. Bidirectional modes expose two sensing links; star exposes one.",
        "Probe missing fraction uses observed sequence interiors only; excludes recording boundaries and cannot attribute causes to RF collision.",
        "Counter deltas cover first-to-last counters reports; UART/frame/capture errors are reported separately.",
        "Transmitter requested/accepted rates use that transmitter epoch and its own counter interval. Driver acceptance is not proof of over-the-air delivery.",
        "Pair follower transmission is conditional on receiving leader requests. Its sequence increments only when it attempts a response, so a low reverse missing fraction alone cannot establish better sampling coverage.",
        "Timing uses receiver node timestamps, not UART arrival times. Interval statistics include loss gaps.",
        "First-second stream/decode errors are separated from later errors; attaching to an active UART can start mid-frame, but timing alone does not prove an error cause.",
        "No motion labels are supplied. Results do not measure sensing accuracy, presence, or fall detection.",
        "Aggregate timing statistics summarize per-run quantiles; they are not quantiles of pooled intervals.",
    ]}


def f(value, places=2):
    return "n/a" if value is None else f"{value:.{places}f}"


def markdown(result):
    out = ["# Acquisition mode comparison", "", "Per-run observations:", "",
           "| Condition | Repeat | Direction | TX requested Hz | TX driver accepted Hz | CSI Hz | Missing interior % | Interval p95 ms | Interval p99 ms | Max gap ms | RSSI mean dBm |",
           "|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for t in result["trials"]:
        for l in t["links"]:
            i = l.get("interarrival_node_us") or {}
            out.append(f'| {t["condition"]} | {t["repeat"]} | {l["direction"]} | '
                       f'{f(l["transmitter_requested_hz"])} | {f(l["transmitter_accepted_hz"])} | {f(l["rate_hz"])} | '
                       f'{f(None if l["interior_missing_fraction"] is None else 100*l["interior_missing_fraction"])} | '
                       f'{f(i.get("p95", 0)/1000) if i else "n/a"} | '
                       f'{f(i.get("p99", 0)/1000) if i else "n/a"} | '
                       f'{f(i.get("max", 0)/1000) if i else "n/a"} | '
                       f'{f((l.get("rssi_dbm") or {}).get("mean"))} |')
        if not t["links"]:
            out.append(f'| {t["condition"]} | {t["repeat"]} | **No CSI links observed** | | | | | | | | |')
    out += ["", "Aggregate rates and loss (mean [min, max] across observed link runs):", "",
            "| Condition | Direction | Repeats | TX requested Hz | TX driver accepted Hz | CSI Hz | Missing interior % |",
            "|---|---|---:|---:|---:|---:|---:|"]
    def range_text(d, scale=1):
        return "n/a" if d is None else f'{f(d["mean"]*scale)} [{f(d["min"]*scale)}, {f(d["max"]*scale)}]'
    for a in result["aggregates"]:
        out.append(f'| {a["condition"]} | {a["direction"]} | {a["trial_count"]} | '
                   f'{range_text(a["transmitter_requested_hz"])} | {range_text(a["transmitter_accepted_hz"])} | '
                   f'{range_text(a["rate_hz"])} | {range_text(a["interior_missing_fraction"], 100)} |')
    out += ["", "Health counts (counter drops use the counter-report interval):", "",
            "| Condition | Repeat | Stream/decode first 1s / later | Missing frames | Missing captures | Pool drops | Export drops | Sealed integrity |",
            "|---|---:|---:|---:|---:|---:|---:|---|"]
    for t in result["trials"]:
        e = t["errors"]
        out.append(f'| {t["condition"]} | {t["repeat"]} | {e["stream_or_decode_first_second"]} / {e["stream_or_decode_after_first_second"]} | '
                   f'{e["frames_missing"]} | {e["captures_missing"]} | {e["pool_drops"]} | '
                   f'{e["export_drops"]} | {t["sealed_integrity_verified"]} |')
        if t["missing_expected_directions"]:
            out.append(f'\nMissing expected directions for {t["condition"]} repeat {t["repeat"]}: '
                       + ", ".join(t["missing_expected_directions"]) + ".\n")
    out += ["", "Interpretation limits:", ""] + [f"- {n}" for n in result["notes"]]
    return "\n".join(out) + "\n"


def svg(result):
    # Dependency-free artifact; same-scale plot of rate and separate loss panel.
    rows = result["aggregates"]
    height = max(250, 100 + 37*len(rows))
    max_rate = max([a["rate_hz"]["max"] for a in rows if a["rate_hz"]] + [1]) * 1.1
    max_loss = max([100*a["interior_missing_fraction"]["max"] for a in rows
                    if a["interior_missing_fraction"]] + [1]) * 1.1
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="{height}" viewBox="0 0 1200 {height}">',
           '<rect width="1200" height="100%" fill="white"/>',
           '<g font-family="sans-serif" font-size="12" fill="#18212c">',
           '<text x="15" y="22" font-size="17">Desk pilot: mean with observed min–max across repeats</text>',
           '<text x="485" y="49">Captured CSI / second</text><text x="885" y="49">Missing probes / observed sequence interior (%)</text>']
    for j, a in enumerate(rows):
        y = 80 + j*37
        label = f'{a["condition"]} | {a["direction"]}'
        out.append(f'<text x="15" y="{y+4}">{html.escape(label)}</text>')
        for metric, scale, x0, color in [(a["rate_hz"], max_rate, 485, "#176b94"),
                                        (a["interior_missing_fraction"], max_loss/100, 885, "#ad4227")]:
            if metric is None:
                continue
            coord = lambda value: x0 + 220*value/scale
            out += [f'<line x1="{x0}" x2="{x0+220}" y1="{y}" y2="{y}" stroke="#e2e5e8"/>',
                    f'<line x1="{coord(metric["min"]):.2f}" x2="{coord(metric["max"]):.2f}" y1="{y}" y2="{y}" stroke="{color}" stroke-width="2"/>',
                    f'<circle cx="{coord(metric["mean"]):.2f}" cy="{y}" r="4" fill="{color}"/>',
                    f'<text x="{x0+230}" y="{y+4}">{metric["mean"]*(100 if x0 == 885 else 1):.2f}</text>']
    out += ['</g></svg>']
    return "\n".join(out)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--registry", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    args = p.parse_args()
    registry = json.loads(args.registry.read_text())
    result = summarize(registry, args.registry.resolve().parent)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
    (args.out_dir / "README.md").write_text(markdown(result))
    (args.out_dir / "comparison.svg").write_text(svg(result))
    print(f'Summarized {len(result["trials"])} trials, {len(result["aggregates"])} condition/directions in {args.out_dir}')


if __name__ == "__main__":
    main()
