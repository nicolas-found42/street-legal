//! Frame-by-frame comparison of two canonical streams, reporting the first
//! divergence of every frame. There is no resynchronization inside a frame and
//! no tolerance: the hooked original's stream is the oracle.
use super::{canon::Canonicalizer, state::hex, Result, CANON_SCHEMA};
use serde_json::{json, Value as Json};
use std::{collections::BTreeSet, io::Read};

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Frames to drop from the front of each stream before comparing, so two
    /// scripted scenes that start at different presented frames can be aligned.
    pub skip_expected: u64,
    pub skip_got: u64,
    /// Upper bound on compared frame pairs.
    pub max_frames: Option<u64>,
}

/// Differences between two events with the same kind, capped for readability.
fn event_differences(expected: &Json, got: &Json) -> Vec<Json> {
    const CAP: usize = 24;
    let mut out = Vec::new();
    let (Some(e), Some(g)) = (expected.as_object(), got.as_object()) else {
        return vec![json!({"field": "event", "expected": expected, "got": got})];
    };
    let keys: BTreeSet<_> = e.keys().chain(g.keys()).collect();
    for key in keys {
        if key == "state" {
            let (es, gs) = (e.get("state"), g.get("state"));
            let empty = serde_json::Map::new();
            let em = es.and_then(Json::as_object).unwrap_or(&empty);
            let gm = gs.and_then(Json::as_object).unwrap_or(&empty);
            let names: BTreeSet<_> = em.keys().chain(gm.keys()).collect();
            for name in names {
                if em.get(name) != gm.get(name) && out.len() < CAP {
                    out.push(json!({
                        "field": format!("state.{name}"),
                        "label": state_label(name),
                        "expected": em.get(name).cloned().unwrap_or(Json::Null),
                        "got": gm.get(name).cloned().unwrap_or(Json::Null),
                    }));
                }
            }
        } else if e.get(key) != g.get(key) && out.len() < CAP {
            out.push(json!({
                "field": key,
                "expected": e.get(key).cloned().unwrap_or(Json::Null),
                "got": g.get(key).cloned().unwrap_or(Json::Null),
            }));
        }
    }
    out
}

fn state_label(key: &str) -> String {
    use super::defaults::*;
    let mut parts = key.split('.');
    let nums: Vec<u32> = key
        .split('.')
        .skip(1)
        .filter_map(|p| p.parse().ok())
        .collect();
    match (parts.next(), nums.as_slice()) {
        (Some("rs"), [n]) => format!("RS {}", render_state_name(*n)),
        (Some("tss"), [s, n]) => format!("stage {s} {}", texture_stage_name(*n)),
        (Some("samp"), [s, n]) => format!("sampler {s} {}", sampler_name(*n)),
        (Some("xf"), [n]) => format!("transform {}", transform_name(*n)),
        _ => key.to_owned(),
    }
}

/// First divergence between two frames, or `None` when identical.
pub fn first_divergence(expected: &[Json], got: &[Json]) -> Option<Json> {
    for (index, (e, g)) in expected.iter().zip(got).enumerate() {
        if e != g {
            let ek = e.get("k").and_then(Json::as_str).unwrap_or("?");
            let gk = g.get("k").and_then(Json::as_str).unwrap_or("?");
            let summary = |v: &Json| {
                let k = v.get("k").and_then(Json::as_str).unwrap_or("?");
                match v.get("m").and_then(Json::as_str) {
                    Some(m) => format!("{k}:{m}"),
                    None => k.to_owned(),
                }
            };
            return Some(json!({
                "event_index": index,
                "expected_event": summary(e),
                "got_event": summary(g),
                "kind_mismatch": ek != gk,
                "differences": event_differences(e, g),
            }));
        }
    }
    if expected.len() != got.len() {
        let index = expected.len().min(got.len());
        let next = |events: &[Json]| {
            events
                .get(index)
                .and_then(|e| e.get("k"))
                .cloned()
                .unwrap_or(Json::Null)
        };
        return Some(json!({
            "event_index": index,
            "length_mismatch": {"expected": expected.len(), "got": got.len()},
            "expected_next": next(expected),
            "got_next": next(got),
        }));
    }
    None
}

fn skip<R: Read>(c: &mut Canonicalizer<R>, n: u64) -> Result<u64> {
    let mut skipped = 0;
    while skipped < n {
        if c.next_frame()?.is_none() {
            break;
        }
        skipped += 1;
    }
    Ok(skipped)
}

pub fn compare<A: Read, B: Read>(
    expected: &mut Canonicalizer<A>,
    got: &mut Canonicalizer<B>,
    options: &Options,
) -> Result<Json> {
    let skipped = (
        skip(expected, options.skip_expected)?,
        skip(got, options.skip_got)?,
    );
    let mut frames = Vec::new();
    let (mut equal, mut diverged) = (0u64, 0u64);
    let (mut missing_expected, mut missing_got) = (0u64, 0u64);
    let mut first_diverged = Json::Null;
    let mut pair = 0u64;
    loop {
        if options.max_frames.is_some_and(|m| pair >= m) {
            break;
        }
        let (e, g) = (expected.next_frame()?, got.next_frame()?);
        let entry = match (e, g) {
            (None, None) => break,
            (Some(e), Some(g)) => {
                let divergence = first_divergence(&e.events, &g.events);
                let same = divergence.is_none() && e.presented == g.presented;
                if same {
                    equal += 1;
                } else {
                    diverged += 1;
                    if first_diverged.is_null() {
                        first_diverged = json!(pair);
                    }
                }
                json!({
                    "pair": pair,
                    "expected_frame": e.index,
                    "got_frame": g.index,
                    "status": if same { "equal" } else { "diverged" },
                    "expected_events": e.events.len(),
                    "got_events": g.events.len(),
                    "expected_sha256": e.sha256(),
                    "got_sha256": g.sha256(),
                    "first_divergence": divergence,
                })
            }
            (Some(e), None) => {
                missing_got += 1;
                diverged += 1;
                if first_diverged.is_null() {
                    first_diverged = json!(pair);
                }
                json!({"pair": pair, "expected_frame": e.index, "status": "missing_in_got", "expected_events": e.events.len()})
            }
            (None, Some(g)) => {
                missing_expected += 1;
                diverged += 1;
                if first_diverged.is_null() {
                    first_diverged = json!(pair);
                }
                json!({"pair": pair, "got_frame": g.index, "status": "missing_in_expected", "got_events": g.events.len()})
            }
        };
        frames.push(entry);
        pair += 1;
    }
    let _ = hex;
    Ok(json!({
        "schema": CANON_SCHEMA,
        "kind": "comparison",
        "options": {
            "skip_expected": options.skip_expected,
            "skip_got": options.skip_got,
            "max_frames": options.max_frames,
        },
        "skipped_frames": {"expected": skipped.0, "got": skipped.1},
        "frames_compared": pair,
        "frames_equal": equal,
        "frames_diverged": diverged,
        "frames_missing_in_got": missing_got,
        "frames_missing_in_expected": missing_expected,
        "first_diverged_pair": first_diverged,
        "equal": diverged == 0 && pair > 0,
        "expected_consumed_sha256": expected.stream_sha256(),
        "got_consumed_sha256": got.stream_sha256(),
        "expected_tallies": tallies_json(&expected.tallies),
        "got_tallies": tallies_json(&got.tallies),
        "frames": frames,
    }))
}

pub fn tallies_json(t: &super::canon::Tallies) -> Json {
    json!({
        "d3d9_records": t.d3d9_records,
        "other_records": t.other_records,
        "by_class": t.classes,
        "by_method": t.methods,
    })
}

#[cfg(test)]
mod tests {
    use super::super::canon::testing::*;
    use super::*;

    fn canon(calls: &[(&str, Vec<slr_calllog::Value>)]) -> Vec<u8> {
        log(calls)
    }
    fn run(a: &[u8], b: &[u8], options: Options) -> Json {
        compare(
            &mut Canonicalizer::new(a).unwrap(),
            &mut Canonicalizer::new(b).unwrap(),
            &options,
        )
        .unwrap()
    }
    fn present() -> (&'static str, Vec<slr_calllog::Value>) {
        ("Present", vec![slr_calllog::Value::Bool(false); 4])
    }
    fn draw(prims: i32) -> (&'static str, Vec<slr_calllog::Value>) {
        (
            "DrawIndexedPrimitive",
            vec![i(4), i(0), i(0), i(4), i(0), i(prims)],
        )
    }

    #[test]
    fn identical_streams_are_equal_per_frame() {
        let a = canon(&[draw(2), present(), draw(3), present()]);
        let r = run(&a, &a, Options::default());
        assert_eq!(r["equal"], true);
        assert_eq!(r["frames_equal"], 2);
    }

    #[test]
    fn first_divergence_names_the_state_key() {
        let a = canon(&[("SetRenderState", vec![i(27), i(1)]), draw(2), present()]);
        let b = canon(&[("SetRenderState", vec![i(27), i(0)]), draw(2), present()]);
        let r = run(&a, &b, Options::default());
        assert_eq!(r["equal"], false);
        assert_eq!(r["first_diverged_pair"], 0);
        let d = &r["frames"][0]["first_divergence"];
        assert_eq!(d["event_index"], 0);
        assert_eq!(d["differences"][0]["field"], "state.rs.27");
        assert_eq!(d["differences"][0]["label"], "RS ALPHABLENDENABLE");
    }

    #[test]
    fn argument_and_length_mismatches_are_reported() {
        let a = canon(&[draw(2), present()]);
        let b = canon(&[draw(5), present()]);
        let r = run(&a, &b, Options::default());
        assert_eq!(
            r["frames"][0]["first_divergence"]["differences"][0]["field"],
            "a"
        );
        let open_one = canon(&[draw(2)]);
        let open_two = canon(&[draw(2), draw(2)]);
        let r = run(&open_one, &open_two, Options::default());
        assert_eq!(
            r["frames"][0]["first_divergence"]["length_mismatch"]["got"],
            2
        );
        let r = run(
            &a,
            &canon(&[draw(2), draw(2), present()]),
            Options::default(),
        );
        assert_eq!(r["frames"][0]["first_divergence"]["kind_mismatch"], true);
    }

    #[test]
    fn missing_frames_and_alignment() {
        let a = canon(&[draw(1), present(), draw(2), present()]);
        let b = canon(&[draw(2), present()]);
        let misaligned = run(&a, &b, Options::default());
        assert_eq!(misaligned["frames_missing_in_got"], 1);
        assert_eq!(misaligned["equal"], false);
        let aligned = run(
            &a,
            &b,
            Options {
                skip_expected: 1,
                ..Options::default()
            },
        );
        assert_eq!(aligned["equal"], true);
    }

    #[test]
    fn an_empty_comparison_is_not_equal() {
        let a = canon(&[]);
        assert_eq!(run(&a, &a, Options::default())["equal"], false);
    }
}
