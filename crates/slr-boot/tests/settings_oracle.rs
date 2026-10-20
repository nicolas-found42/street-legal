//! Original427bf0 arithmetic/writes; supplied Config getters and refresh seam.
use serde_json::{json, Value};
use slr_boot::settings::{Event, Kind, Settings, Source};
use std::collections::VecDeque;
struct Inputs(VecDeque<Value>);
impl Source for Inputs {
    fn read(&mut self, kind: Kind, name: &'static str) -> Result<u32, String> {
        let next = self.0.pop_front().expect("original getter");
        assert_eq!(
            next["kind"],
            match kind {
                Kind::Int => "int",
                Kind::Float => "float",
                Kind::Has => "has",
            }
        );
        assert_eq!(next["name"], name);
        Ok(if kind == Kind::Float {
            (next["value"].as_f64().unwrap() as f32).to_bits()
        } else {
            next["value"].as_u64().unwrap() as u32
        })
    }
}
#[test]
fn settings_original_rows() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/boot/settings.jsonl");
    let Ok(rows) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: settings_original_rows");
        return;
    };
    let mut count = 0;
    for line in rows.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let mut source = Inputs(row["calls"].as_array().unwrap().clone().into());
        let mut settings = Settings::default();
        // Verify optional absence leaves supplied existing words intact.
        for address in [
            0x617728, 0x618d64, 0x618d68, 0x60e028, 0x60e020, 0x60e024, 0x63c83c, 0x63c7a8,
            0x63c7ac, 0x60e050, 0x60e054, 0x60e058, 0x60e05c, 0x60e014, 0x60e018, 0x60e01c,
        ] {
            settings.globals.insert(address, 0xaabbccdd);
        }
        settings.apply(&mut source).unwrap();
        assert_eq!(source.0.pop_front().unwrap(), json!({"refresh":"0x4fc590"}));
        assert!(source.0.is_empty());
        let writes: Vec<_> = settings
            .events
            .iter()
            .filter_map(|e| {
                if let Event::Write { address, bits } = e {
                    Some(json!({"address":format!("{address:#x}"),"bits":bits,"size":4}))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(writes, row["writes"].as_array().unwrap().to_owned());
        if !row["optional"].as_bool().unwrap() {
            for address in [
                0x617728, 0x618d64, 0x618d68, 0x60e028, 0x60e020, 0x60e024, 0x63c83c, 0x63c7a8,
                0x63c7ac, 0x60e050, 0x60e054, 0x60e058, 0x60e05c, 0x60e014, 0x60e018, 0x60e01c,
            ] {
                assert_eq!(settings.globals[&address], 0xaabbccdd);
            }
        }
        assert_eq!(
            settings.events.last(),
            Some(&Event::Refresh { address: 0x4fc590 })
        );
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn absent_backend_refresh_original_rows() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/boot/refresh.jsonl");
    let Ok(rows) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: absent_backend_refresh_original_rows");
        return;
    };
    let mut count = 0;
    for line in rows.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        if row["device"].as_bool().unwrap() {
            continue;
        }
        let mut settings = Settings::default();
        for (i, address) in [0x64adac, 0x64adb0].into_iter().enumerate() {
            settings
                .globals
                .insert(address, row["before"][i].as_u64().unwrap() as u32);
        }
        settings.finish_refresh(false).unwrap();
        assert!(settings.events.is_empty());
        assert_eq!(row["events"].as_array().unwrap().len(), 0);
        for (i, address) in [0x64adac, 0x64adb0].into_iter().enumerate() {
            assert_eq!(
                settings.globals[&address] as u64,
                row["after"][i].as_u64().unwrap()
            );
        }
        count += 1;
    }
    assert_eq!(count, 2);
}
