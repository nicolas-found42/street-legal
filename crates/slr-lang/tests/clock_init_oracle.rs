//! Replay original counter initialization and subsequent reads on one clock.

use serde_json::Value;
use slr_lang::clock::{CounterClock, CounterState, PerformanceCounters};
use slr_x87::{Precision, Rounding};
use std::path::PathBuf;

struct Queries<'a> {
    operation: &'a Value,
    order: Vec<&'static str>,
}

impl PerformanceCounters for Queries<'_> {
    fn query_frequency(&mut self, destination: &mut i64) -> bool {
        self.order.push("QueryPerformanceFrequency");
        if let Some(value) = self.operation["frequency"].as_i64() {
            *destination = value;
        }
        self.operation["status"].as_u64().unwrap() != 0
    }

    fn query_counter(&mut self, destination: &mut i64) -> bool {
        self.order.push("QueryPerformanceCounter");
        if let Some(value) = self.operation["counter"].as_i64() {
            *destination = value;
        }
        self.operation["status"].as_u64().unwrap() != 0
    }
}

#[test]
fn initialized_counter_sequences_match_original() {
    let path = std::env::var_os("SLR_CLOCK_INIT_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/counter-clock/clock_init.jsonl")
        });
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: initialized_counter_sequences_match_original");
        return;
    };
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 13, "incomplete clock initialization corpus");
    let mut identities = std::collections::BTreeSet::new();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        assert!(identities.insert(id.to_owned()), "duplicate sequence");
        let initial = row["initial"].as_array().unwrap();
        assert_eq!(initial.len(), 3);
        let mut clock = CounterClock::from_state(CounterState {
            frequency: initial[0].as_i64().unwrap(),
            start: initial[1].as_i64().unwrap(),
            now: initial[2].as_i64().unwrap(),
        });
        let operations = row["operations"].as_array().unwrap();
        let results = row["result"].as_array().unwrap();
        let expected_kinds: &[&str] = if id.ends_with("-007f") {
            &["init"]
        } else if id == "reinitialize" {
            &["init", "read", "init", "read"]
        } else {
            &["init", "read"]
        };
        let kinds: Vec<_> = operations
            .iter()
            .map(|operation| operation["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, expected_kinds, "{id} incomplete operation sequence");
        assert_eq!(operations.len(), results.len());
        for (operation, expected) in operations.iter().zip(results) {
            assert_eq!(operation["kind"], expected["kind"]);
            let mut source = Queries {
                operation,
                order: Vec::new(),
            };
            match operation["kind"].as_str().unwrap() {
                "init" => assert_eq!(
                    u64::from(clock.initialize(&mut source)),
                    expected["value"].as_u64().unwrap(),
                    "{id} initialization result"
                ),
                "read" => {
                    let value =
                        clock.read_milliseconds(&mut source, Precision::F32, Rounding::NearestEven);
                    let raw: String = value
                        .to_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    assert_eq!(raw, expected["value"].as_str().unwrap(), "{id} return");
                }
                unknown => panic!("unknown operation {unknown}"),
            }
            let state = clock.state();
            let expected_state: Vec<_> = expected["state"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            assert_eq!(
                [state.frequency, state.start, state.now].as_slice(),
                expected_state,
                "{id} state"
            );
            let expected_order: Vec<_> = expected["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| call["api"].as_str().unwrap())
                .collect();
            assert_eq!(source.order, expected_order, "{id} queries");
        }
    }
    let wanted: std::collections::BTreeSet<_> = [
        "init-ordinary-007f",
        "init-nonintegral-007f",
        "init-large-cancel-007f",
        "init-wide-delta-007f",
        "init-signed-007f",
        "init-freq-zero-007f",
        "init-zero-zero-007f",
        "init-failed-retains-007f",
        "init-failed-writes-007f",
        "init-read",
        "failed-init-read",
        "reinitialize",
        "failed-write-init-read",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(identities, wanted, "missing initialization sequence");
}
