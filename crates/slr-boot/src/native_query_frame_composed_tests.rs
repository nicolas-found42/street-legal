//! Composed original caller evidence; resource/VM/config bodies stay declared
//! collaborators. Runtime class dispatch receives its real inherited CALL.
use super::*;
use crate::native_instance::tests::{check, Controlled};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const REGS: [(&str, Reg); 7] = [
    ("eax", Reg::Eax),
    ("ebx", Reg::Ebx),
    ("ecx", Reg::Ecx),
    ("edx", Reg::Edx),
    ("esi", Reg::Esi),
    ("edi", Reg::Edi),
    ("ebp", Reg::Ebp),
];
fn word(v: &Value) -> u32 {
    v.as_u64().unwrap().try_into().unwrap()
}
fn hex(v: &Value) -> Vec<u8> {
    let text = v.as_str().unwrap();
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
fn digest(path: &std::path::Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}
fn validate_state(state: &Value) {
    assert_eq!(state["heap"].as_array().unwrap().len(), 0x5000 / 4);
    assert_eq!(state["stack"].as_array().unwrap().len(), 1024);
    assert_eq!(state["registers"].as_object().unwrap().len(), 8);
    for name in ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"] {
        word(&state["registers"][name]);
    }
    let globals = declared_globals();
    assert_eq!(state["globals"].as_object().unwrap().len(), globals.len());
    for address in globals {
        word(&state["globals"][address.to_string()]);
    }
}
fn declared_globals() -> Vec<u32> {
    let mut globals = vec![
        0x765f60, 0x63c86c, 0x7686f0, 0x7686f8, 0x63c870, 0x63c874, 0x6439c0, 0x610a58, 0x610a5c,
        0x6495a4,
    ];
    globals.extend((0x636338..0x636408).step_by(4));
    globals
}
fn inventory() -> Vec<Value> {
    let mut inventory = vec![];
    for kind in ["player", "cursor"] {
        for fill in [0, 0xa5] {
            for parent in [false, true] {
                inventory.push(
                    json!({"mode":"ready", "composition":kind, "fill":fill, "parent":parent}),
                );
            }
        }
    }
    inventory
}
fn validate_inventory(data: &Value) {
    let inventory = inventory();
    assert_eq!(data["inventory"], json!(inventory));
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), inventory.len());
    for (row, expected) in rows.iter().zip(inventory) {
        assert_eq!(row["case"], expected);
    }
}

struct Composed<'a> {
    base: Controlled<'a>,
    nodes: u32,
}
impl Composed<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String> {
        if address == 0x427210 {
            self.base.record(m, address, args);
            register(m, this.unwrap(), args[0], self)?;
            return Ok(s::reg(m, Reg::Eax));
        }
        if matches!(address, 0x47b220 | 0x4625f0) {
            self.base.record(m, address, args);
            // Exercise the production target-identity/class dispatch. Its
            // unrelated resource/script collaborators cannot execute here.
            return Host::call(
                &mut Runtime(&mut crate::native_install::Detached),
                m,
                address,
                args,
                this,
            );
        }
        if address == 0x54f560 && args == [0x1c] {
            return s::function_cdecl(m, args, |m| {
                self.base.record(m, address, args);
                assert!(self.nodes < 3);
                let p = word(&self.base.row["layout"]["heap_address"]) + 0x4800 + self.nodes * 0x20;
                self.nodes += 1;
                Ok(p)
            });
        }
        if address == 0x419860 || address == word(&self.base.row["postcreate"]) {
            return s::function(m, args, this, |m| {
                self.base.record(m, address, args);
                Ok(if address == 0x419860 {
                    word(&self.base.row["layout"]["template_payload"])
                } else {
                    0
                })
            });
        }
        crate::native_instance::Host::call(&mut self.base, m, address, args, this)
    }
}
impl crate::native_instance::Host for Composed<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String> {
        self.call(m, address, args, this)
    }
}
impl Host for Composed<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String> {
        self.call(m, address, args, this)
    }
}

#[test]
fn actual_instance_query_registration_composition_preserves_inherited_frames() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::var_os("SLR_QUERY_FRAME_COMPOSED")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/boot/native-query-frame-composed-original.json"));
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
            "required composed fixture absent: {}",
            path.display()
        );
        eprintln!("SLR_ORACLE_SKIPPED: native_query_frame_composed_probe.py");
        return;
    };
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, relative) in [
        (
            "producer_sha256",
            "re/oracle/native_query_frame_composed_probe.py",
        ),
        ("source_sha256", "re/oracle/native_instance_probe.py"),
        ("oracle_sha256", "re/oracle/oracle.py"),
        ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
    ] {
        assert_eq!(data[field], digest(&root.join(relative)));
    }
    assert_eq!(data["schema"], 1);
    assert_eq!(data["fpu_cw"], 0x7f);
    assert_eq!(data["profile"]["entry"], 0x53a2a0);
    assert_eq!(data["global_addresses"], json!(declared_globals()));
    assert_eq!(
        data["profile_sha256"],
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&data["profile"]).unwrap())
        )
    );
    validate_inventory(&data);
    let rows = data["rows"].as_array().unwrap();
    let mut failures = vec![];
    for (index, row) in rows.iter().enumerate() {
        // Retain/report every case's first divergence; a failure still fails
        // the complete test and cannot turn a passing subset into evidence.
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_eq!(row["layout"]["heap_address"], 0x20000000);
            assert_eq!(row["layout"]["heap_words"], 0x5000 / 4);
            assert_eq!(row["layout"]["stack_address"], 0x7feff000);
            assert_eq!(row["layout"]["stack_words"], 1024);
            validate_state(&row["before"]);
            validate_state(&row["after"]);
            for event in row["events"].as_array().unwrap() {
                validate_state(event);
            }
            let kind = row["case"]["composition"].as_str().unwrap();
            assert_eq!(
                row["native_table"],
                if kind == "player" { 0x5f12f0 } else { 0x5f11e4 }
            );
            assert_eq!(
                row["registration_nodes"],
                if kind == "player" { 2 } else { 1 }
            );
            assert_eq!(row["esp_delta"], 0);
            assert_eq!(row["fpu_cw"], 0x7f);
            assert_eq!(row["fpu_depth"], 0);
            assert!(row["import_calls"].as_array().unwrap().is_empty());
            let mut m = Space::new();
            for id in row["input_page_ids"].as_array().unwrap() {
                let page = &data["input_pages"][id.as_str().unwrap()];
                let bytes = hex(&page["bytes"]);
                assert_eq!(bytes.len(), 4096);
                assert_eq!(page["sha256"], format!("{:x}", Sha256::digest(&bytes)));
                for (i, byte) in bytes.into_iter().enumerate() {
                    m.w8(word(&page["address"]) + i as u32, byte);
                }
            }
            for (field, base) in [("heap", 0x20000000), ("stack", 0x7feff000)] {
                for (i, value) in row["before"][field].as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, word(value));
                }
            }
            for (address, value) in row["before"]["globals"].as_object().unwrap() {
                m.w32(address.parse().unwrap(), word(value));
            }
            let args: Vec<_> = row["args"].as_array().unwrap().iter().map(word).collect();
            let entry = &row["contexts"][0]["registers"];
            for (name, _) in REGS {
                assert_ne!(word(&entry[name]), 0, "nonzero inherited driver {name}");
            }
            let mut host = Composed {
                base: Controlled { row, next: 0 },
                nodes: 0,
            };
            s::function(&mut m, &[], None, |m| {
                for (name, register) in REGS {
                    s::set_reg(m, register, word(&entry[name]));
                }
                s::set_sp(m, word(&entry["esp"]) + (args.len() as u32 + 1) * 4);
                s::call(m, 0x7fff0000, &args, None, |m| {
                    let result = crate::native_instance::create(
                        m,
                        args.clone().try_into().unwrap(),
                        &mut host,
                    )
                    .unwrap_or_else(|e| panic!("composed case{index} {:?}: {e}", row["case"]));
                    assert_eq!(result, word(&row["result"]));
                    check(
                        m,
                        &row["after"],
                        &row["layout"],
                        &format!("composed case{index} final"),
                    );
                });
            });
            assert_eq!(host.base.next, row["events"].as_array().unwrap().len());
            assert_eq!(host.nodes, word(&row["registration_nodes"]));
        }));
        if attempt.is_err() {
            failures.push((index, row["case"].clone()));
        }
    }
    assert!(
        failures.is_empty(),
        "composed caller failures: {failures:?}"
    );
}

#[test]
fn composed_fixture_guards_reject_omitted_cases_and_shortened_states() {
    let cases = inventory();
    let document = json!({"inventory":cases, "rows":cases.iter().map(|case|json!({"case":case})).collect::<Vec<_>>()});
    validate_inventory(&document);
    for mutation in 0..3 {
        let mut broken = document.clone();
        match mutation {
            0 => {
                broken["rows"].as_array_mut().unwrap().pop();
            }
            1 => {
                broken["rows"][7] = broken["rows"][0].clone();
            }
            _ => {
                broken["inventory"][0]["fill"] = json!(0xa5);
            }
        }
        assert!(std::panic::catch_unwind(|| validate_inventory(&broken)).is_err());
    }
    let mut registers = serde_json::Map::new();
    for name in ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"] {
        registers.insert(name.into(), json!(0));
    }
    let globals: serde_json::Map<String, Value> = declared_globals()
        .into_iter()
        .map(|address| (address.to_string(), json!(0)))
        .collect();
    let state = json!({"heap":vec![0u32;0x5000/4], "stack":vec![0u32;1024], "registers":registers, "globals":globals});
    validate_state(&state);
    for field in ["heap", "stack", "registers", "globals"] {
        let mut broken = state.clone();
        match field {
            "heap" | "stack" => {
                broken[field].as_array_mut().unwrap().pop();
            }
            "registers" => {
                broken[field].as_object_mut().unwrap().remove("edi");
            }
            _ => {
                broken[field]
                    .as_object_mut()
                    .unwrap()
                    .remove(&0x636338u32.to_string());
            }
        }
        assert!(
            std::panic::catch_unwind(|| validate_state(&broken)).is_err(),
            "shortened {field} accepted"
        );
    }
}
