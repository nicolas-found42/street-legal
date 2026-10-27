use super::*;
use crate::native_vm_fpu;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
fn observe(p: u32, n: u32, v: u32) {
    if p < 0x7fe00000 {
        STORES.with(|s| s.borrow_mut().push((p, n, v)));
    }
}
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}
fn decode(blobs: &[&Value]) -> Vec<Vec<u8>> {
    let script="import base64,hashlib,json,struct,sys,zlib\nfor b in json.load(sys.stdin):\n raw=base64.b64decode(b['zlib_base64'],validate=True); z=zlib.decompressobj(); d=z.decompress(raw,2097153); assert len(d)<=2097152 and not z.unconsumed_tail; d+=z.flush(); assert z.eof and not z.unused_data and len(d)==b['bytes'] and hashlib.sha256(d).hexdigest()==b['sha256']; sys.stdout.buffer.write(struct.pack('<I',len(d))+d)";
    let mut child = std::process::Command::new("python3")
        .args(["-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(blobs).unwrap())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut cursor = 0;
    let result = blobs
        .iter()
        .map(|blob| {
            let n = u32::from_le_bytes(out.stdout[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            assert_eq!(n, word(&blob["bytes"]) as usize);
            let data = out.stdout[cursor..cursor + n].to_vec();
            cursor += n;
            assert_eq!(
                format!("{:x}", Sha256::digest(&data)),
                blob["sha256"].as_str().unwrap()
            );
            data
        })
        .collect();
    assert_eq!(cursor, out.stdout.len());
    result
}
struct Snapshot {
    regions: Vec<(u32, Vec<u8>)>,
    heap_ptr: u32,
    registers: [u32; 8],
    fpu: Value,
}
fn snapshots(values: &[&Value]) -> Vec<Snapshot> {
    let mut blobs = vec![];
    for value in values {
        blobs.extend([&value["heap"], &value["stack"]]);
        blobs.extend(
            value["image"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| &p["data"]),
        );
    }
    let mut bytes = decode(&blobs).into_iter();
    values
        .iter()
        .map(|value| {
            let mut regions = vec![
                (0x20000000, bytes.next().unwrap()),
                (0x7fefe000, bytes.next().unwrap()),
            ];
            assert_eq!(regions[0].1.len(), 0x40000);
            assert_eq!(regions[1].1.len(), 0x2000);
            for image in value["image"].as_array().unwrap() {
                regions.push((word(&image["base"]), bytes.next().unwrap()));
            }
            Snapshot {
                regions,
                heap_ptr: word(&value["heap_ptr"]),
                registers: ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"]
                    .map(|r| word(&value["registers"][r])),
                fpu: value["fpu"].clone(),
            }
        })
        .collect()
}
fn compare(m: &Space, v: &Snapshot, fpu: &native_vm_fpu::State) {
    for (p, bytes) in &v.regions {
        assert!(
            m.first_difference(*p, bytes).is_none(),
            "memory {p:x}: {:?}",
            m.first_difference(*p, bytes)
        );
    }
    assert_eq!(m.heap_ptr(), v.heap_ptr);
    for (reg, want) in [
        Reg::Eax,
        Reg::Ebx,
        Reg::Ecx,
        Reg::Edx,
        Reg::Esi,
        Reg::Edi,
        Reg::Ebp,
    ]
    .into_iter()
    .zip(v.registers)
    {
        assert_eq!(r(m, reg), want, "register {}", reg as u32);
    }
    assert_eq!(stack::sp(m), v.registers[7]);
    assert_eq!(u32::from(fpu.control()), word(&v.fpu["control"]));
    assert_eq!(u32::from(fpu.status()), word(&v.fpu["status"]));
    assert_eq!(u32::from(fpu.tags()), word(&v.fpu["tags"]));
    for (actual, want) in fpu
        .registers()
        .into_iter()
        .zip(v.fpu["registers"].as_array().unwrap())
    {
        let b = actual.to_bytes();
        assert_eq!(
            u64::from_le_bytes(b[..8].try_into().unwrap()),
            want[0].as_u64().unwrap()
        );
        assert_eq!(
            u16::from_le_bytes(b[8..].try_into().unwrap()) as u64,
            want[1].as_u64().unwrap()
        );
    }
}
struct Env<'a> {
    events: &'a [Value],
    states: &'a [Snapshot],
    next: usize,
    manager: u32,
    ids: [u32; 2],
    resources: [u32; 2],
    missing: bool,
    fpu: native_vm_fpu::State,
}
impl Env<'_> {
    fn check(&mut self, m: &Space, kind: &str, args: Value) {
        assert_eq!(self.events[self.next]["kind"], kind);
        assert_eq!(self.events[self.next]["args"], args);
        compare(m, &self.states[self.next], &self.fpu);
        self.next += 1;
    }
}
impl Allocator for Env<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.check(m, "allocate", json!([n]));
        Ok(m.alloc(n))
    }
    fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
        self.check(m, "reallocate", json!([p, n]));
        let q = m.alloc(n);
        for i in 0..old.min(n) {
            m.w8(q + i, m.r8(p + i));
        }
        Ok(q)
    }
}
impl resources::Environment for Env<'_> {
    fn package_id(
        &mut self,
        m: &mut Space,
        manager: u32,
        package: u32,
        id: u32,
    ) -> Result<u32, String> {
        assert_eq!(manager, self.manager);
        assert_eq!(r(m, Reg::Ecx), manager);
        self.check(m, "package", json!([package, id]));
        let text = (0..11).map(|i| m.r8(package + i)).collect::<Vec<_>>();
        assert_eq!(text, b"system.rpk\0");
        assert!(id == 7 || id == 8);
        Ok(if self.missing {
            0
        } else {
            self.ids[usize::from(id == 7)]
        })
    }
    fn resolve(
        &mut self,
        m: &mut Space,
        manager: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<u32, String> {
        assert_eq!(manager, self.manager);
        self.check(m, "resolve", json!([id, kind, flags]));
        assert_eq!(flags, 0);
        let i = usize::from(id == self.ids[1]);
        assert_eq!(id, self.ids[i]);
        assert_eq!(kind, [7, 5][i]);
        Ok(self.resources[i])
    }
}
struct Observers;
impl Drop for Observers {
    fn drop(&mut self) {
        super::super::set_test_store_observer(None);
        crate::native_vm_signature::set_test_store_observer(None);
        crate::native_vm_test_store::set(None);
        crate::native_vm_class_context::set_test_store_observer(None);
        crate::native_vm_bootstrap::set_test_store_observer(None);
        crate::native_vm_metadata::names::set_test_store_observer(None);
    }
}
#[test]
fn original_catalog_resource_suffix_complete_retained_context() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(data) = crate::oracle_fixture::file(
        &root.join("re/out/boot/native-vm-registry-resources-original.json"),
    ) else {
        return;
    };
    let v: Value = serde_json::from_slice(&std::fs::read(data).unwrap()).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(
        v["exe_sha256"],
        format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        )
    );
    for (name, hash) in v["producer_sources"].as_object().unwrap() {
        assert_eq!(
            hash.as_str().unwrap(),
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    assert_eq!(v["rows"].as_array().unwrap().len(), 8);
    for (row_index, row) in v["rows"].as_array().unwrap().iter().enumerate() {
        assert_eq!(row["seed"], [606, 20261023][row_index / 4]);
        assert_eq!(
            row["mode"],
            ["empty", "replace", "same", "missing"][row_index % 4]
        );
        let events = row["events"].as_array().unwrap();
        let mut values = vec![&row["before"]];
        values.extend(events.iter().map(|e| &e["state"]));
        values.push(&row["after"]);
        let states = snapshots(&values);
        let before = &states[0];
        let after = states.last().unwrap();
        let mut m = Space::new();
        let images = row["image_inputs"].as_array().unwrap();
        let image_bytes = decode(&images.iter().map(|p| &p["data"]).collect::<Vec<_>>());
        for (image, bytes) in images.iter().zip(image_bytes) {
            let base = word(&image["base"]);
            assert!((0x400000..0x7c0000).contains(&base));
            for (i, b) in bytes.into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
        }
        for (p, bytes) in &before.regions {
            for (i, b) in bytes.iter().enumerate() {
                m.w8(*p + i as u32, *b);
            }
        }
        m.set_heap_ptr(before.heap_ptr);
        // The controlled driver enters its source CALL; captured words are test
        // comparison inputs only. Production has no capture loader or direct fallback.
        let outer = stack::enter_cdecl(&mut m, &[]);
        for (reg, value) in [
            Reg::Eax,
            Reg::Ebx,
            Reg::Ecx,
            Reg::Edx,
            Reg::Esi,
            Reg::Edi,
            Reg::Ebp,
        ]
        .into_iter()
        .zip(before.registers)
        {
            s(&mut m, reg, value);
        }
        stack::set_sp(&mut m, before.registers[7] + 4);
        stack::begin_call(&mut m, 0x30000028, &[], None);
        let fp = &before.fpu;
        let registers = std::array::from_fn(|i| {
            let mut b = [0u8; 10];
            b[..8].copy_from_slice(&fp["registers"][i][0].as_u64().unwrap().to_le_bytes());
            b[8..].copy_from_slice(&(word(&fp["registers"][i][1]) as u16).to_le_bytes());
            slr_x87::X87::from_bytes(b)
        });
        let fpu = native_vm_fpu::State::from_saved(
            word(&fp["control"]) as u16,
            word(&fp["status"]) as u16,
            word(&fp["tags"]) as u16,
            registers,
        );
        let mut env = Env {
            events,
            states: &states[1..states.len() - 1],
            next: 0,
            manager: word(&row["manager"]),
            ids: [word(&row["ids"][0]), word(&row["ids"][1])],
            resources: [word(&row["resources"][0]), word(&row["resources"][1])],
            missing: row["mode"] == "missing",
            fpu,
        };
        STORES.with(|s| s.borrow_mut().clear());
        super::super::set_test_store_observer(Some(observe));
        crate::native_vm_signature::set_test_store_observer(Some(observe));
        crate::native_vm_test_store::set(Some(observe));
        crate::native_vm_class_context::set_test_store_observer(Some(observe));
        crate::native_vm_bootstrap::set_test_store_observer(Some(observe));
        crate::native_vm_metadata::names::set_test_store_observer(Some(observe));
        let _observers = Observers;
        let catalog = begin_catalog(&mut m, &mut env).unwrap();
        catalog.finish(&mut m, &mut env).unwrap();
        assert_eq!(env.next, events.len());
        compare(&m, after, &env.fpu);
        let expected = row["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (word(&s[0]), word(&s[1]), word(&s[2])))
            .collect::<Vec<_>>();
        STORES.with(|s| { let actual=s.borrow(); let first=actual.iter().zip(&expected).position(|(a,b)|a!=b); assert!(first.is_none(), "ordered stores row{row_index} first {first:?}: actual {:?}, expected {:?}; lengths {}/{}", first.and_then(|i|actual.get(i)), first.and_then(|i|expected.get(i)), actual.len(),expected.len());assert_eq!(actual.len(),expected.len(),"store count row{row_index}");});
        stack::leave(&mut m, outer);
    }
}
