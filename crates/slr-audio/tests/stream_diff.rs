//! Data-free adversarial tests of the canonical stream reader and comparator.

use serde_json::{json, Map, Value};
use slr_audio::diff::{first_divergence, Divergence};
use slr_audio::stream::{Event, Stream, StreamError, SCHEMA};

fn event(i: u64, call: &str, this: &str, args: Value, ret: u64, out: Value) -> Event {
    Event {
        i,
        call: call.into(),
        this: this.into(),
        args,
        ret,
        out,
        meta: json!({"icount": 1, "site": "0x559944"}),
    }
}

fn sample() -> Stream {
    let mut s = Stream::new(Map::new());
    s.events.push(event(
        0,
        "DirectSoundCreate",
        "api",
        json!({"guid_given": false}),
        0,
        json!({"object": "ds"}),
    ));
    s.events.push(event(
        1,
        "IDirectSoundBuffer.SetVolume",
        "buf#1",
        json!({"volume": -450}),
        0,
        json!({}),
    ));
    s.events.push(event(
        2,
        "IDirectSoundBuffer.SetFrequency",
        "buf#1",
        json!({"frequency": 22050}),
        0,
        json!({}),
    ));
    s
}

#[test]
fn identical_streams_have_no_divergence() {
    assert_eq!(first_divergence(&sample(), &sample()), None);
}

#[test]
fn meta_is_evidence_and_never_compared() {
    let mut other = sample();
    other.events[1].meta = json!({"icount": 999, "site": "0x1", "region_sha256": "00"});
    assert_eq!(first_divergence(&sample(), &other), None);
}

#[test]
fn argument_difference_names_the_field() {
    let mut other = sample();
    other.events[1].args = json!({"volume": -451});
    assert_eq!(
        first_divergence(&sample(), &other),
        Some(Divergence::Field {
            index: 1,
            path: "args.volume".into(),
            expected: "-450".into(),
            got: "-451".into()
        })
    );
}

#[test]
fn result_object_and_call_differences_are_divergences() {
    let mut a = sample();
    a.events[0].out = json!({"object": "ds"});
    let mut b = a.clone();
    b.events[0].out = json!({"object": "buf#0"});
    assert!(
        matches!(first_divergence(&a, &b), Some(Divergence::Field { path, .. }) if path == "out.object")
    );
    let mut c = sample();
    c.events[2].ret = 0x8878001e;
    assert!(
        matches!(first_divergence(&sample(), &c), Some(Divergence::Field { path, .. }) if path == "ret")
    );
    let mut d = sample();
    d.events[1].call = "IDirectSoundBuffer.SetPan".into();
    assert!(
        matches!(first_divergence(&sample(), &d), Some(Divergence::Field { path, .. }) if path == "call")
    );
}

#[test]
fn extra_and_missing_fields_are_divergences() {
    let mut other = sample();
    other.events[1].args = json!({"volume": -450, "pan": 0});
    assert!(
        matches!(first_divergence(&sample(), &other), Some(Divergence::Field { path, got, .. }) if path == "args.pan" && got == "0")
    );
}

#[test]
fn a_common_prefix_is_not_a_pass() {
    let mut shorter = sample();
    shorter.events.pop();
    assert_eq!(
        first_divergence(&sample(), &shorter),
        Some(Divergence::Length {
            common: 2,
            expected: 3,
            got: 2
        })
    );
    assert_eq!(
        first_divergence(&shorter, &sample()),
        Some(Divergence::Length {
            common: 2,
            expected: 2,
            got: 3
        })
    );
}

#[test]
fn render_and_parse_round_trip() {
    let stream = sample();
    assert_eq!(Stream::parse(&stream.render()).unwrap(), stream);
}

#[test]
fn reader_rejects_malformed_streams() {
    assert!(matches!(Stream::parse(""), Err(StreamError::Schema(_))));
    assert!(matches!(
        Stream::parse("{\"schema\":\"other/1\"}\n"),
        Err(StreamError::Schema(_))
    ));
    let header = format!("{{\"schema\":\"{SCHEMA}\"}}\n");
    let gap = format!(
        "{header}{}\n",
        json!({"i": 1, "call": "x", "this": "ds", "args": {}, "ret": 0, "out": {}, "meta": {}})
    );
    assert!(matches!(
        Stream::parse(&gap),
        Err(StreamError::Ordinal {
            expected: 0,
            found: 1,
            ..
        })
    ));
    let unknown = format!(
        "{header}{}\n",
        json!({"i": 0, "call": "x", "this": "ds", "args": {}, "ret": 0, "out": {}, "meta": {}, "extra": 1})
    );
    assert!(matches!(
        Stream::parse(&unknown),
        Err(StreamError::Shape(_))
    ));
    let not_json = format!("{header}{{oops\n");
    assert!(matches!(
        Stream::parse(&not_json),
        Err(StreamError::Json { line: 2, .. })
    ));
}
