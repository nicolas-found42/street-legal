//! Round-trip tests (M0-10 acceptance 1): every record kind serializes and
//! deserializes to identical bytes; a full log re-reads identically.
//!
//! All fixtures are built in-test; no data files are read or committed.

use slr_calllog::{
    D3d9CallKind, DirectInputKind, DirectSoundKind, FileOpKind, FrameVehicleState, LogReader,
    LogRecord, LogWriter, MemRecorder, NullRecorder, Recorder, Value, VmSnapshot,
};
use std::io::BufReader;

/// One of every value shape, including float bit patterns that decimal
/// text would mangle (NaN payload, -0.0, subnormal, infinities).
fn all_values() -> Vec<Value> {
    vec![
        Value::Int(0),
        Value::Int(-1),
        Value::Int(i32::MIN),
        Value::Int(i32::MAX),
        Value::Long(0),
        Value::Long(i64::MIN),
        Value::Long(i64::MAX),
        Value::float(0.0),
        Value::float(-0.0),
        Value::float(1.0),
        Value::FloatBits(0x7fc0_0001), // NaN with payload
        Value::FloatBits(0x0000_0001), // smallest subnormal
        Value::float(f32::INFINITY),
        Value::float(f32::NEG_INFINITY),
        Value::double(0.0),
        Value::double(-0.0),
        Value::DoubleBits(0x7ff8_0000_0000_0001), // NaN with payload
        Value::DoubleBits(0x0000_0000_0000_0001),
        Value::double(f64::INFINITY),
        Value::Str(String::new()),
        Value::Str("java.game.GameType".to_owned()),
        Value::Str("héllo wörld — 日本語".to_owned()),
        Value::Bytes(vec![]),
        Value::Bytes(vec![0, 1, 2, 255]),
        Value::Bytes((0..=255u8).collect()),
        Value::Bool(false),
        Value::Bool(true),
    ]
}

/// One of every record kind.
fn all_records() -> Vec<LogRecord> {
    vec![
        LogRecord::NativeCall {
            class: "java.game.GameType".to_owned(),
            name: "addCall".to_owned(),
            args: all_values(),
            result: Some(Value::Int(42)),
        },
        LogRecord::NativeCall {
            class: "java.game.parts.Part".to_owned(),
            name: "reset".to_owned(),
            args: vec![],
            result: None, // void native
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::DeviceState,
            method: "SetRenderState".to_owned(),
            args: vec![Value::Int(7), Value::Long(0x0100_0000)],
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::Draw,
            method: "DrawIndexedPrimitive".to_owned(),
            args: vec![
                Value::Int(4),
                Value::Int(0),
                Value::Int(120),
                Value::float(1.0),
            ],
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::Resource,
            method: "CreateTexture".to_owned(),
            args: vec![
                Value::Int(256),
                Value::Int(256),
                Value::Str("D3DFMT_A8R8G8B8".to_owned()),
            ],
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::VertexFormat,
            method: "SetFVF".to_owned(),
            args: vec![Value::Long(0x1c2)],
        },
        LogRecord::D3d9Call {
            kind: D3d9CallKind::Present,
            method: "Present".to_owned(),
            args: vec![],
        },
        LogRecord::DirectSoundCall {
            kind: DirectSoundKind::Buffer,
            method: "Play".to_owned(),
            args: vec![Value::Int(0), Value::Bool(true)],
        },
        LogRecord::DirectSoundCall {
            kind: DirectSoundKind::Spatial,
            method: "SetPosition".to_owned(),
            args: vec![Value::float(1.5), Value::float(-2.25), Value::float(0.0)],
        },
        LogRecord::DirectSoundCall {
            kind: DirectSoundKind::Capture,
            method: "DirectSoundCaptureCreate".to_owned(),
            args: vec![],
        },
        LogRecord::DirectInput {
            kind: DirectInputKind::Keyboard,
            report: vec![0x1eu8; 32],
        },
        LogRecord::DirectInput {
            kind: DirectInputKind::Mouse,
            report: vec![10, 0, 250, 3],
        },
        LogRecord::DirectInput {
            kind: DirectInputKind::Gamepad,
            report: (0..=255u8).collect(),
        },
        LogRecord::FileIo {
            op: FileOpKind::Open,
            path: "script/scr/start.class".to_owned(),
            detail: vec![Value::Str("rb".to_owned())],
        },
        LogRecord::FileIo {
            op: FileOpKind::Read,
            path: "script/scr/start.class".to_owned(),
            detail: vec![
                Value::Long(4096),
                Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]),
            ],
        },
        LogRecord::FileIo {
            op: FileOpKind::Write,
            path: "saves/slot1.json".to_owned(),
            detail: vec![Value::Bytes(b"{}".to_vec())],
        },
        LogRecord::FileIo {
            op: FileOpKind::Close,
            path: "saves/slot1.json".to_owned(),
            detail: vec![],
        },
        LogRecord::FileIo {
            op: FileOpKind::Find,
            path: "script/scr/*.class".to_owned(),
            detail: vec![Value::Str("start.class".to_owned())],
        },
        LogRecord::VmSnapshot(VmSnapshot {
            frame: 120,
            pc: 0x0041_bf42,
            control_word: 0x027f,
            stack: all_values(),
        }),
        LogRecord::FrameVehicle(FrameVehicleState::from_floats(
            120,
            [10.5, 0.35, -44.125],
            [1.5707964, -0.0, 0.0],
            [22.75, 0.0, -3.5],
            5850.0,
            3,
        )),
    ]
}

fn write_log(records: &[LogRecord]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = LogWriter::create(&mut buf).unwrap();
        for r in records {
            w.record(r).unwrap();
        }
        w.flush().unwrap();
    }
    buf
}

fn read_all(bytes: &[u8]) -> Vec<LogRecord> {
    let mut reader = LogReader::open(BufReader::new(bytes)).unwrap();
    let mut out = Vec::new();
    while let Some(r) = reader.next_record().unwrap() {
        out.push(r.clone());
    }
    out
}

#[test]
fn header_is_magic_plus_version() {
    let buf = write_log(&[]);
    assert_eq!(&buf, b"SLRLOG\x01\x00");
}

#[test]
fn every_value_round_trips() {
    for v in all_values() {
        let record = LogRecord::NativeCall {
            class: "c".to_owned(),
            name: "m".to_owned(),
            args: vec![v.clone()],
            result: Some(v.clone()),
        };
        let back = read_all(&write_log(std::slice::from_ref(&record)));
        assert_eq!(back, vec![record]);
    }
}

#[test]
fn every_record_kind_round_trips() {
    let records = all_records();
    assert_eq!(read_all(&write_log(&records)), records);
}

#[test]
fn full_log_rereads_identically_byte_for_byte() {
    let records = all_records();
    let first = write_log(&records);
    let reread = read_all(&first);
    assert_eq!(reread, records);
    // Re-encoding the decoded log must reproduce the exact bytes, so two
    // emitters (Rust, Python, C) can be compared with `cmp`.
    let second = write_log(&reread);
    assert_eq!(first, second);
}

#[test]
fn float_bit_patterns_survive() {
    // Decimal text would print both as "NaN" and both as "0"; the log must
    // keep every bit.
    let records = vec![LogRecord::NativeCall {
        class: "c".to_owned(),
        name: "m".to_owned(),
        args: vec![
            Value::FloatBits(0x7fc0_0001),
            Value::FloatBits(0x7fc0_0002),
            Value::FloatBits(0x8000_0000), // -0.0
            Value::FloatBits(0x0000_0000), // +0.0
        ],
        result: None,
    }];
    let back = read_all(&write_log(&records));
    assert_eq!(back, records);
    assert_ne!(
        back[0],
        read_all(&write_log(&[LogRecord::NativeCall {
            class: "c".to_owned(),
            name: "m".to_owned(),
            args: vec![Value::FloatBits(0x7fc0_0002)],
            result: None,
        }]))[0]
    );
}

#[test]
fn recorders_agree_with_writer() {
    let records = all_records();
    // MemRecorder buffers, then serializes identically.
    let mut mem = MemRecorder::new();
    for r in &records {
        mem.push(r.clone()).unwrap();
    }
    assert_eq!(mem.records(), records.as_slice());
    let mut buf = Vec::new();
    mem.write_to(&mut buf).unwrap();
    assert_eq!(buf, write_log(&records));
    // Direct convenience methods route correctly too.
    let mut mem2 = MemRecorder::new();
    mem2.native_call("c", "m", vec![Value::Int(1)], None)
        .unwrap();
    mem2.d3d9_call(D3d9CallKind::Draw, "Draw", vec![]).unwrap();
    mem2.direct_sound_call(DirectSoundKind::Buffer, "Play", vec![])
        .unwrap();
    mem2.direct_input(DirectInputKind::Mouse, vec![1, 2])
        .unwrap();
    mem2.file_io(FileOpKind::Close, "f", vec![]).unwrap();
    mem2.vm_snapshot(VmSnapshot {
        frame: 1,
        pc: 2,
        control_word: 3,
        stack: vec![],
    })
    .unwrap();
    mem2.frame_vehicle(FrameVehicleState::from_floats(
        1, [0.0; 3], [0.0; 3], [0.0; 3], 0.0, 0,
    ))
    .unwrap();
    assert_eq!(mem2.len(), 7);
    assert_eq!(
        read_all(&{
            let mut b = Vec::new();
            mem2.write_to(&mut b).unwrap();
            b
        })
        .len(),
        7
    );
    // NullRecorder counts and drops.
    let mut null = NullRecorder::new();
    for r in &records {
        null.push(r.clone()).unwrap();
    }
    assert_eq!(null.count(), records.len() as u64);
}

#[test]
fn unknown_record_kind_fails_loudly() {
    let mut buf = b"SLRLOG\x01\x00".to_vec();
    buf.push(0x7f); // no such record kind
    buf.extend_from_slice(&0u32.to_le_bytes());
    let mut reader = LogReader::open(BufReader::new(buf.as_slice())).unwrap();
    let err = reader.next_record().unwrap_err();
    assert!(matches!(
        err,
        slr_calllog::LogError::UnknownRecordKind { tag: 0x7f, .. }
    ));
}

#[test]
fn unknown_value_tag_fails_loudly() {
    // A native call whose first arg carries tag 0x77.
    let mut body = Vec::new();
    body.extend_from_slice(&1u32.to_le_bytes()); // class len
    body.push(b'c');
    body.extend_from_slice(&1u32.to_le_bytes()); // name len
    body.push(b'm');
    body.extend_from_slice(&1u32.to_le_bytes()); // one arg
    body.push(0x77); // no such value tag
    let mut buf = b"SLRLOG\x01\x00".to_vec();
    buf.push(0x00);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    let mut reader = LogReader::open(BufReader::new(buf.as_slice())).unwrap();
    let err = reader.next_record().unwrap_err();
    assert!(matches!(
        err,
        slr_calllog::LogError::UnknownValueTag { tag: 0x77, .. }
    ));
}

#[test]
fn bad_magic_and_version_rejected() {
    let err = match LogReader::open(BufReader::new(b"BADMAGIC".as_slice())) {
        Err(e) => e,
        Ok(_) => panic!("expected bad-magic error"),
    };
    assert!(matches!(err, slr_calllog::LogError::BadMagic { .. }));
    let err = match LogReader::open(BufReader::new(b"SLRLOG\x09\x00".as_slice())) {
        Err(e) => e,
        Ok(_) => panic!("expected version error"),
    };
    assert!(matches!(
        err,
        slr_calllog::LogError::UnsupportedVersion { found: 9, .. }
    ));
}

#[test]
fn truncated_body_is_an_error_not_eof() {
    let mut buf = write_log(&all_records());
    buf.truncate(buf.len() - 3); // cut inside the last record body
    let mut reader = LogReader::open(BufReader::new(buf.as_slice())).unwrap();
    let mut count = 0;
    let err = loop {
        match reader.next_record() {
            Ok(Some(_)) => count += 1,
            Ok(None) => panic!("expected truncation error, got clean EOF"),
            Err(e) => break e,
        }
    };
    assert!(count > 0);
    assert!(matches!(err, slr_calllog::LogError::Truncated { .. }));
}
