//! Resource eligibility before original4255e0 creates a VM callback thread.
//! This bounded prefix does not supply the callback/interpreter's retained
//! machine frames. The native ABI boundary remains a separate dependency.
use slr_physics::space::Space;

pub fn target(
    m: &mut Space,
    envelope: u32,
    mut access: impl FnMut(&mut Space, u32) -> Result<u32, String>,
) -> Result<Option<u32>, String> {
    if m.r32(envelope + 0x50) == 0 {
        return Ok(None);
    }
    let secondary = m.r32(envelope + 0x44);
    if secondary == 0 || access(m, secondary)? == 0 {
        return Ok(None);
    }
    //425701 rereads the envelope after loading. A callback can replace or
    // clear the script; zero here still reaches original408a30, unlike the
    // initial absent-script fast return. Keep Some(0) distinct from None.
    Ok(Some(m.r32(envelope + 0x50)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    fn validate(row: &Value) -> Result<(), &'static str> {
        let word = |v: &Value| v.as_u64().is_some_and(|v| v <= u64::from(u32::MAX));
        let words = |v: &Value| v.as_array().is_some_and(|a| a.iter().all(word));
        if row["before"].as_array().map(Vec::len) != Some(0xc0)
            || row["after"].as_array().map(Vec::len) != Some(0xc0)
            || !words(&row["before"])
            || !words(&row["after"])
            || row["events"].as_array().is_none_or(|events| {
                events.iter().any(|e| {
                    e["heap"].as_array().map(Vec::len) != Some(0xc0)
                        || !words(&e["heap"])
                        || !words(&e["args"])
                        || !word(&e["this"])
                })
            })
        {
            return Err("incomplete original heap observation");
        }
        if ["script", "envelope", "secondary", "payload", "kind"]
            .iter()
            .any(|key| !word(&row[*key]))
        {
            return Err("invalid machine word");
        }
        if row["ready"] == true {
            if row["stop"] != "emulation stopped early at eip=0x425643"
                || !word(&row["target_script"])
            {
                return Err("missing declared VM-setup endpoint");
            }
        } else if row["ready"] != false || !row["stop"].is_null() || !row["target_script"].is_null()
        {
            return Err("missing ordinary no-callback endpoint");
        }
        Ok(())
    }

    #[test]
    fn original_script_access_prefix_matches_complete_heaps_and_live_target() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-script-access-original.json"))
                .expect("run native_script_access_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["entry"], 0x4255e0);
        assert_eq!(data["fpu_cw"], 0x7f);
        let profile = json!({
            "name":"4255e0-resource-prefix-v1",
            "endpoint":"425643 before VM-thread setup, or ordinary zero return",
            "comparison":"complete caller heap, ordered resource calls and callback eligibility/live script",
            "controlled":["resource minimum virtual","5447d0","resource payload virtual"],
            "unclaimed":["VM execution","retained stack","CPU registers","physical devices"]
        });
        assert_eq!(data["profile"], profile, "declared comparison boundary");
        assert_eq!(
            data["profile_sha256"],
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&profile).unwrap())
            )
        );
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_script_access_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        let expected_inventory = [0, 0x13572468]
            .into_iter()
            .flat_map(|script| {
                [false, true].into_iter().flat_map(move |present| {
                    [1, 8].into_iter().flat_map(move |kind| {
                        [
                            "ready",
                            "pending",
                            "no-payload",
                            "minimum-replaces-script",
                            "prepare-replaces-script",
                            "payload-clears-script",
                        ]
                        .into_iter()
                        .map(move |mode| (script, present, kind, mode.to_owned()))
                    })
                })
            })
            .collect::<std::collections::BTreeSet<_>>();
        let actual_inventory = rows
            .iter()
            .map(|row| {
                (
                    row["script"].as_u64().unwrap(),
                    row["secondary_present"].as_bool().unwrap(),
                    row["kind"].as_u64().unwrap(),
                    row["mode"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(rows.len(), expected_inventory.len());
        assert_eq!(actual_inventory, expected_inventory);
        for (case, row) in rows.iter().enumerate() {
            validate(row).unwrap();
            let word = |key: &str| row[key].as_u64().unwrap() as u32;
            let mut m = Space::new();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut events = Vec::new();
            let mut record = |m: &Space, name: &str, raw: u32, args: &[u32]| {
                events.push(json!({"name":name,"this":raw,"args":args,
                    "heap":(0..0xc0).map(|i|m.r32(0x20000000+i*4)).collect::<Vec<_>>()}));
            };
            let result = target(&mut m, word("envelope"), |m, raw| {
                assert_eq!(raw, word("secondary"));
                let mode = row["mode"].as_str().unwrap();
                if m.r32(raw + 0x4c) != 1 {
                    record(m, "minimum", raw, &[0x3f800000]);
                    if mode == "minimum-replaces-script" {
                        m.w32(word("envelope") + 0x50, 0x24681357);
                    }
                }
                record(m, "prepare", raw, &[0x80000000, 0, 0]);
                if mode == "prepare-replaces-script" {
                    m.w32(word("envelope") + 0x50, 0x35792468);
                }
                if mode == "pending" {
                    return Ok(0);
                }
                record(m, "payload", raw, &[0x3f800000]);
                if mode == "payload-clears-script" {
                    m.w32(word("envelope") + 0x50, 0);
                }
                Ok(if mode == "no-payload" {
                    0
                } else {
                    word("payload")
                })
            })
            .unwrap();
            assert_eq!(
                result.map(u64::from),
                row["target_script"].as_u64(),
                "case{case} eligibility/live target"
            );
            assert_eq!(
                events,
                *row["events"].as_array().unwrap(),
                "case{case} complete callback-time heaps"
            );
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "case{case} complete heap word{i}"
                );
            }
        }
    }

    #[test]
    fn incomplete_script_access_observations_are_rejected() {
        let row = json!({"before":vec![0;0xc0],"after":vec![0;0xc0],"events":[],
            "script":0,"envelope":0x20000000,"secondary":0x20000100,"payload":0x20000200,"kind":1,
            "ready":false,"stop":null,"target_script":null});
        validate(&row).unwrap();
        for field in ["before", "after"] {
            let mut malformed = row.clone();
            malformed[field] = json!([]);
            assert!(validate(&malformed).is_err());
        }
        let mut malformed = row.clone();
        malformed["events"] = json!([{"heap":[]}]);
        assert!(validate(&malformed).is_err());
        let mut malformed = row;
        malformed["ready"] = json!(true);
        assert!(validate(&malformed).is_err());
    }

    #[test]
    fn contradictory_endpoints_and_oversized_words_cannot_pass() {
        let row = json!({"before":vec![0;0xc0],"after":vec![0;0xc0],"events":[],
            "script":0,"secondary_present":false,"kind":1,"mode":"ready",
            "envelope":0x20000000,"secondary":0x20000100,"payload":0x20000200,
            "ready":false,"stop":null,"target_script":null});
        validate(&row).unwrap();
        let mut malformed = row.clone();
        malformed["target_script"] = json!(1);
        assert!(validate(&malformed).is_err(), "target requires VM endpoint");
        for field in ["before", "after"] {
            let mut malformed = row.clone();
            malformed[field][0] = json!(0x100000000u64);
            assert!(validate(&malformed).is_err(), "oversized {field} word");
        }
        let mut malformed = row;
        malformed["script"] = json!(0x100000000u64);
        assert!(validate(&malformed).is_err(), "oversized pointer word");
    }
}
