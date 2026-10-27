//! Declared Python device model only; no original sound-library or host output claim.
use serde_json::Value;
use sha2::{Digest, Sha256};
use slr_audio::device::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
fn u(v: &Value) -> u32 {
    v.as_u64().or_else(|| v.as_i64().map(|v| v as u64)).unwrap() as u32
}
fn fmt(v: &Value) -> Option<WaveFormat> {
    (!v.is_null()).then(|| WaveFormat {
        tag: u(&v["tag"]) as u16,
        channels: u(&v["channels"]) as u16,
        rate: u(&v["rate"]),
        avg_bytes_per_sec: u(&v["avg_bytes_per_sec"]),
        block_align: u(&v["block_align"]) as u16,
        bits: u(&v["bits"]) as u16,
    })
}
fn id(v: &Value) -> BufId {
    BufId(
        v.as_str()
            .unwrap()
            .strip_prefix("buf#")
            .unwrap()
            .parse()
            .unwrap(),
    )
}
fn run(d: &mut DirectSound, op: &Value, last: &mut Option<Locked>) -> Result<(), DsError> {
    match op["op"].as_str().unwrap() {
        "clock" => d.set_clock_ms(op["ms"].as_u64().unwrap()),
        "create" => {
            d.create()?;
        }
        "caps" => {
            d.get_caps(u(&op["size"]))?;
        }
        "coop" => {
            d.set_cooperative_level(op["window"].as_u64().unwrap(), u(&op["level"]))?;
        }
        "create_buffer" => {
            let v = &op["desc"];
            let mut guid = [0; 16];
            let raw = v["guid"].as_str().unwrap();
            for (i, b) in guid.iter_mut().enumerate() {
                *b = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).unwrap();
            }
            let _ = d.create_sound_buffer(&BufferDesc {
                size: u(&v["size"]),
                flags: u(&v["flags"]),
                bytes: u(&v["bytes"]),
                reserved: u(&v["reserved"]),
                format: fmt(&v["format"]),
                guid_3d_algorithm: guid,
            })?;
        }
        "duplicate" => {
            let _ = d.duplicate_sound_buffer(id(&op["src"]))?;
        }
        "set_format" => {
            d.set_format(id(&op["buf"]), fmt(&op["format"]))?;
        }
        "scalar" => {
            let b = id(&op["buf"]);
            let a = op["args"].as_array().unwrap();
            match op["method"].as_str().unwrap() {
                "SetVolume" => {
                    d.set_volume(b, u(&a[0]) as i32)?;
                }
                "SetPan" => {
                    d.set_pan(b, u(&a[0]) as i32)?;
                }
                "SetFrequency" => {
                    d.set_frequency(b, u(&a[0]))?;
                }
                "Play" => {
                    d.play(b, u(&a[0]), u(&a[1]), u(&a[2]))?;
                }
                "Stop" => {
                    d.stop(b)?;
                }
                "SetCurrentPosition" => {
                    d.set_current_position(b, u(&a[0]))?;
                }
                method => panic!("unhandled scalar {method}"),
            }
        }
        "get" => {
            let b = id(&op["buf"]);
            match op["method"].as_str().unwrap() {
                "GetCurrentPosition" => {
                    d.get_current_position(b, true, true)?;
                }
                "GetStatus" => {
                    d.get_status(b)?;
                }
                "GetVolume" => {
                    d.get_volume(b)?;
                }
                "GetPan" => {
                    d.get_pan(b)?;
                }
                "GetFrequency" => {
                    d.get_frequency(b)?;
                }
                m => panic!("unhandled get {m}"),
            }
        }
        "get_format" => {
            d.get_format(id(&op["buf"]), u(&op["size"]))?;
        }
        "get_caps" => {
            d.get_caps_buffer(id(&op["buf"]), u(&op["size"]))?;
        }
        "lock" => {
            let slots = std::array::from_fn(|i| op["slots"][i].as_bool().unwrap());
            if let Ok(mut lock) = d.lock(
                id(&op["buf"]),
                u(&op["offset"]),
                u(&op["bytes"]),
                u(&op["flags"]),
                slots,
            )? {
                if !slots[3] {
                    lock.len2 = 0;
                }
                *last = Some(lock);
            }
        }
        "unlock" => {
            let mut back = UnlockArgs {
                first_offset: last.map(|v| v.offset),
                len1: last.map_or(0, |v| v.len1),
                second_offset: last.filter(|v| v.len2 != 0).map(|_| 0),
                len2: last.map_or(0, |v| v.len2),
            };
            if op["mode"] == "wrong" {
                back.len1 ^= 4;
            }
            d.unlock_regions(id(&op["buf"]), back)?;
        }
        "qi" => {
            let iid = match op["iid"].as_str().unwrap() {
                "listener" => Iid::Listener3d,
                "3d" => Iid::Buffer3d,
                "other" => Iid::Other(std::array::from_fn(|i| i as u8)),
                v => panic!("iid {v}"),
            };
            d.query_interface(id(&op["buf"]), &iid)?;
        }
        "addref" => {
            d.add_ref_buffer(id(&op["buf"]))?;
        }
        "release" => {
            d.release_buffer(id(&op["buf"]))?;
        }
        "release_ds" => {
            d.release_device()?;
        }
        v => panic!("unhandled op {v}"),
    }
    Ok(())
}
#[test]
fn device_sequences_match_declared_python_model() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::var_os("SLR_DSOUND_DEVICE_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/audio/vectors/device.jsonl"));
    let bytes = match std::fs::read_to_string(&path) {
        Ok(v) => v,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none() =>
        {
            eprintln!("SLR_ORACLE_SKIPPED: declared device model vectors");
            return;
        }
        Err(e) => panic!("{}: {e}", path.display()),
    };
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["oracle_kind"], "declared-device-model");
    assert_eq!(manifest["exe_code_executed"], false);
    assert_eq!(manifest["seed"], 20261026);
    assert_eq!(manifest["cases_per_profile"], 60);
    assert_eq!(manifest["resource_arena_bytes"], 0x10000000u64);
    assert_eq!(
        manifest["vector_sha256"],
        format!("{:x}", Sha256::digest(bytes.as_bytes()))
    );
    let required = [
        "re/headless/com.py",
        "re/headless/com_tables.py",
        "re/headless/d3d9_device.py",
        "re/headless/dsound_platform.py",
        "re/headless/dsound_trace.py",
        "re/headless/make_boot_profile.py",
        "re/headless/startup.py",
        "re/oracle/dsound_device_vectors.py",
    ];
    let sources = manifest["sources"].as_object().unwrap();
    assert_eq!(
        sources.keys().map(String::as_str).collect::<Vec<_>>(),
        required
    );
    for source in required {
        assert_eq!(
            sources[source],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join(source)).unwrap())
            )
        );
    }
    let profiles = manifest["profiles"].as_object().unwrap();
    assert_eq!(
        profiles.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        [
            "profile-lead0/platform-profile.json",
            "profile-lead10/platform-profile.json",
            "profile-lead37/platform-profile.json"
        ]
        .into_iter()
        .collect()
    );
    for (profile, digest) in profiles {
        assert_eq!(
            *digest,
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(path.parent().unwrap().join(profile)).unwrap())
            )
        );
    }
    let caps="dwFlags dwMinSecondarySampleRate dwMaxSecondarySampleRate dwPrimaryBuffers dwMaxHwMixingAllBuffers dwMaxHwMixingStaticBuffers dwMaxHwMixingStreamingBuffers dwFreeHwMixingAllBuffers dwFreeHwMixingStaticBuffers dwFreeHwMixingStreamingBuffers dwMaxHw3DAllBuffers dwMaxHw3DStaticBuffers dwMaxHw3DStreamingBuffers dwFreeHw3DAllBuffers dwFreeHw3DStaticBuffers dwFreeHw3DStreamingBuffers dwTotalHwMemBytes dwFreeHwMemBytes dwMaxContigFreeHwMemBytes dwUnlockTransferRateHwBuffers dwPlayCpuOverheadSwBuffers dwReserved1 dwReserved2".split_whitespace().collect::<Vec<_>>();
    let rows = bytes
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 180);
    let mut events = 0;
    let mut identities = BTreeSet::new();
    for row in rows {
        let identity = row["id"].as_str().unwrap();
        assert!(identities.insert(identity.to_owned()));
        let (lead, n) = identity
            .strip_prefix("lead")
            .unwrap()
            .split_once('-')
            .unwrap();
        assert!(n.parse::<u32>().unwrap() < 60);
        assert!([0, 10, 37].contains(&lead.parse::<u32>().unwrap()));
        assert_eq!(
            u(&row["sound"]["write_lead_ms"]),
            lead.parse::<u32>().unwrap()
        );
        let sound = &row["sound"];
        let mut d = DirectSound::new(
            DsProfile {
                caps: std::array::from_fn(|i| u(&sound["caps"][caps[i]])),
                write_lead_ms: u(&sound["write_lead_ms"]),
            },
            vec![row["window"].as_u64().unwrap()],
        )
        .with_resource_budget(0x10000000);
        let mut last = None;
        let mut stopped = None;
        for (i, op) in row["ops"].as_array().unwrap().iter().enumerate() {
            if let Err(error) = run(&mut d, op, &mut last) {
                stopped = Some((i, error));
                break;
            }
        }
        assert_eq!(
            stopped.as_ref().map(|(i, _)| *i),
            row["stop"]["op"].as_u64().map(|v| v as usize),
            "{} stop {stopped:?}",
            row["id"]
        );
        let actual = d
            .events()
            .iter()
            .map(|event| {
                let mut v = event.to_json();
                v.as_object_mut().unwrap().remove("meta");
                v
            })
            .collect::<Vec<_>>();
        let expected = row["events"].as_array().unwrap();
        for i in 0..actual.len().max(expected.len()) {
            assert_eq!(actual.get(i), expected.get(i), "{} event {i}", row["id"]);
        }
        events += actual.len();
    }
    assert!(events > 2000);
}

#[test]
fn device_calls_require_created_and_unreleased_com_object() {
    let mut d = DirectSound::new(
        DsProfile {
            caps: [0; 23],
            write_lead_ms: 10,
        },
        vec![0x10040],
    );
    let desc = BufferDesc {
        size: 36,
        flags: 0xE0,
        bytes: 8820,
        reserved: 0,
        format: Some(WaveFormat {
            tag: 1,
            channels: 1,
            rate: 22050,
            avg_bytes_per_sec: 44100,
            block_align: 2,
            bits: 16,
        }),
        guid_3d_algorithm: [0; 16],
    };
    assert!(d.get_caps(96).is_err());
    assert!(d.set_cooperative_level(0x10040, 2).is_err());
    assert!(d.create_sound_buffer(&desc).is_err());
    assert!(d.duplicate_sound_buffer(BufId(0)).is_err());
    assert!(d.release_device().is_err());
    assert!(d.events().is_empty());
    d.create().unwrap();
    d.set_cooperative_level(0x10040, 2).unwrap();
    let buffer = d.create_sound_buffer(&desc).unwrap().unwrap();
    d.release_device().unwrap();
    let events = d.events().len();
    assert!(d.get_caps(96).is_err());
    assert!(d.set_cooperative_level(0x10040, 2).is_err());
    assert!(d.create_sound_buffer(&desc).is_err());
    assert!(d.duplicate_sound_buffer(buffer).is_err());
    assert!(d.release_device().is_err());
    assert_eq!(d.events().len(), events);
    assert_eq!(d.buffer_count(), 1);
    // Child COM objects keep their own lifetimes after their parent's release.
    assert!(d.get_status(buffer).is_ok());
}
