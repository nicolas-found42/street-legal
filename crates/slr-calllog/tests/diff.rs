//! Diff acceptance (M0-10 acceptance 2): the tool reports the first
//! divergence on a synthetic pair, plus clean length-mismatch output.
//!
//! The synthetic pair is built in-test (no data files): `base` is a small
//! boot-like prefix, `one_ulp` flips a single float bit in record #2, and
//! `truncated` drops the tail.

use slr_calllog::{
    first_divergence, D3d9CallKind, DiffOutcome, Divergence, FrameVehicleState, LogRecord, Value,
};

fn base_log() -> Vec<LogRecord> {
    vec![
        LogRecord::NativeCall {
            class: "java.game.GameType".to_owned(),
            name: "addCall".to_owned(),
            args: vec![Value::Int(1)],
            result: None,
        },
        LogRecord::NativeCall {
            class: "java.game.Vector3".to_owned(),
            name: "length".to_owned(),
            args: vec![Value::float(3.0), Value::float(4.0)],
            result: Some(Value::float(5.0)),
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::Draw,
            method: "DrawIndexedPrimitive".to_owned(),
            args: vec![Value::Int(120)],
        },
        LogRecord::FrameVehicle(FrameVehicleState::from_floats(
            7,
            [10.5, 0.35, -44.125],
            [0.0, 0.0, 0.0],
            [22.75, 0.0, -3.5],
            5850.0,
            3,
        )),
    ]
}

#[test]
fn identical_logs_report_ok() {
    let base = base_log();
    assert_eq!(
        first_divergence(&base, &base.clone()),
        DiffOutcome::Ok { records: 4 }
    );
}

#[test]
fn first_divergence_points_at_record_ordinal_kind_and_field() {
    let expected = base_log();
    let mut got = base_log();
    // One ulp in the second float argument of record #1 (Vector3.length).
    got[1] = LogRecord::NativeCall {
        class: "java.game.Vector3".to_owned(),
        name: "length".to_owned(),
        args: vec![Value::float(3.0), Value::FloatBits(0x4080_0001)],
        result: Some(Value::float(5.0)),
    };
    let outcome = first_divergence(&expected, &got);
    let Divergence::FieldMismatch {
        ordinal,
        kind,
        fields,
    } = (match outcome {
        DiffOutcome::Diverged(d) => d,
        DiffOutcome::Ok { .. } => panic!("expected a divergence"),
    })
    else {
        panic!("expected a field mismatch");
    };
    assert_eq!(ordinal, 1);
    assert_eq!(kind, "native_call");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].path, "args[1]");
    // 4.0f32 is 0x40800000; the mutant is 0x40800001.
    assert_eq!(fields[0].expected, "f32:0x40800000");
    assert_eq!(fields[0].got, "f32:0x40800001");
    // The Display rendering is what the bin prints; pin it.
    let text = DiffOutcome::Diverged(Divergence::FieldMismatch {
        ordinal,
        kind,
        fields: fields.clone(),
    })
    .to_string();
    assert_eq!(
        text,
        "DIVERGENCE at record #1 (native_call):\n  field args[1]: expected f32:0x40800000, got f32:0x40800001"
    );
}

#[test]
fn kind_mismatch_reported() {
    let expected = base_log();
    let mut got = base_log();
    got[2] = LogRecord::NativeCall {
        class: "c".to_owned(),
        name: "m".to_owned(),
        args: vec![],
        result: None,
    };
    assert_eq!(
        first_divergence(&expected, &got),
        DiffOutcome::Diverged(Divergence::KindMismatch {
            ordinal: 2,
            expected: "d3d9_call",
            got: "native_call",
        })
    );
}

#[test]
fn length_mismatch_reported_cleanly() {
    let expected = base_log();
    let truncated = base_log()[..3].to_vec();
    assert_eq!(
        first_divergence(&expected, &truncated),
        DiffOutcome::Diverged(Divergence::LengthMismatch {
            common: 3,
            expected_len: Some(4),
            got_len: Some(3),
        })
    );
    let text = DiffOutcome::Diverged(Divergence::LengthMismatch {
        common: 3,
        expected_len: Some(4),
        got_len: Some(3),
    })
    .to_string();
    assert_eq!(
        text,
        "DIVERGENCE after record #3: length mismatch: expected 4 records, got 3 records"
    );
}

#[test]
fn first_of_two_divergences_wins() {
    let expected = base_log();
    let mut got = base_log();
    got[0] = LogRecord::NativeCall {
        class: "java.game.GameType".to_owned(),
        name: "addCall".to_owned(),
        args: vec![Value::Int(2)], // divergence #1
        result: None,
    };
    got[3] = LogRecord::FrameVehicle(FrameVehicleState::from_floats(
        7,
        [10.5, 0.35, -44.125],
        [0.0, 0.0, 0.0],
        [22.75, 0.0, -3.5],
        5851.0, // divergence #2, must not be reported
        3,
    ));
    match first_divergence(&expected, &got) {
        DiffOutcome::Diverged(Divergence::FieldMismatch { ordinal, .. }) => {
            assert_eq!(ordinal, 0);
        }
        other => panic!("expected the record-#0 divergence, got {other:?}"),
    }
}
