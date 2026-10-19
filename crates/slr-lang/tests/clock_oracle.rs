//! Original 0x5516c0 counter-clock observations, kept in ignored local JSONL.

use serde_json::Value;
use slr_lang::clock::{CounterClock, CounterState, PerformanceCounters};
use slr_x87::{Precision, Rounding};
use std::path::PathBuf;

struct Queries<'a> {
    row: &'a Value,
    order: Vec<&'static str>,
}

impl PerformanceCounters for Queries<'_> {
    fn query_frequency(&mut self, destination: &mut i64) -> bool {
        self.order.push("QueryPerformanceFrequency");
        if let Some(value) = self.row["write_frequency"].as_i64() {
            *destination = value;
        }
        self.row["status"].as_u64().unwrap() != 0
    }

    fn query_counter(&mut self, destination: &mut i64) -> bool {
        self.order.push("QueryPerformanceCounter");
        if let Some(value) = self.row["write_now"].as_i64() {
            *destination = value;
        }
        self.row["status"].as_u64().unwrap() != 0
    }
}

#[test]
fn counter_clock_matches_original_queries_and_x87_return() {
    let path = std::env::var_os("SLR_CLOCK_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/counter-clock/clock.jsonl")
        });
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: counter_clock_matches_original_queries_and_x87_return");
        return;
    };
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 108, "incomplete clock vectors");
    let mut identities = std::collections::BTreeSet::new();
    for row in rows {
        let input = &row["input"];
        let mut clock = CounterClock::from_state(CounterState {
            frequency: input["frequency"].as_i64().unwrap(),
            start: input["start"].as_i64().unwrap(),
            now: input["now"].as_i64().unwrap(),
        });
        let mut queries = Queries {
            row: input,
            order: Vec::new(),
        };
        let cw = row["cw"].as_u64().unwrap();
        let precision = match (cw >> 8) & 3 {
            0 => Precision::F32,
            2 => Precision::F64,
            3 => Precision::Ext,
            _ => panic!("unsupported clock precision"),
        };
        let rounding = match (cw >> 10) & 3 {
            0 => Rounding::NearestEven,
            1 => Rounding::Down,
            2 => Rounding::Up,
            _ => Rounding::Zero,
        };
        assert!(
            identities.insert(row["id"].as_str().unwrap().to_owned()),
            "duplicate clock row"
        );
        let actual = clock.read_milliseconds(&mut queries, precision, rounding);
        let hex: String = actual
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            hex,
            row["result"]["st0_raw"].as_str().unwrap(),
            "{}",
            row["name"]
        );
        let state = clock.state();
        assert_eq!(
            [state.frequency, state.start, state.now],
            row["result"]["globals"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect::<Vec<_>>()
                .as_slice(),
            "{} state",
            row["name"]
        );
        let expected: Vec<_> = row["result"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["api"].as_str().unwrap())
            .collect();
        assert_eq!(queries.order, expected, "{} queries", row["name"]);
    }
    for name in [
        "ordinary",
        "nonintegral",
        "large-cancel",
        "wide-delta",
        "signed",
        "freq-zero",
        "zero-zero",
        "failed-retains",
        "failed-writes",
    ] {
        for pc in [0_u64, 0x200, 0x300] {
            for rc in [0_u64, 0x400, 0x800, 0xc00] {
                assert!(
                    identities.contains(&format!("{name}-{:04x}", 0x7f | pc | rc)),
                    "missing clock case"
                );
            }
        }
    }
}
