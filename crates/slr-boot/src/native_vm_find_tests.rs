use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
const HEAP: u32 = 0x20000000;
const IMAGE: [(u32, usize); 2] = [(0x766000, 0x3000), (0x777000, 0x6000)];
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
fn record(p: u32, n: u32, v: u32) {
    if (HEAP..0x24000000).contains(&p) {
        assert!(p + n <= HEAP + 0x80000);
    }
    if p >= HEAP && p + n <= HEAP + 0x80000
        || IMAGE
            .iter()
            .any(|(base, len)| p >= *base && p + n <= base + *len as u32)
    {
        STORES.with(|s| s.borrow_mut().push((p, n, v)));
    } else {
        assert!(
            p >= 0x7feff000 && p + n <= 0x7ff00000,
            "unexpected source store {p:x}/{n}"
        );
    }
}
struct Observe;
impl Drop for Observe {
    fn drop(&mut self) {
        set_test_store_observer(None);
    }
}
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}
fn regs() -> [(Reg, &'static str); 7] {
    [
        (Reg::Eax, "eax"),
        (Reg::Ebx, "ebx"),
        (Reg::Ecx, "ecx"),
        (Reg::Edx, "edx"),
        (Reg::Esi, "esi"),
        (Reg::Edi, "edi"),
        (Reg::Ebp, "ebp"),
    ]
}
fn decode(blobs: &[&Value]) -> Vec<Vec<u8>> {
    let script="import base64,hashlib,json,struct,sys,zlib\nfor b in json.load(sys.stdin):\n raw=base64.b64decode(b['zlib_base64'],validate=True);z=zlib.decompressobj();d=z.decompress(raw,524289);assert len(d)<=524288 and not z.unconsumed_tail;d+=z.flush();assert z.eof and not z.unused_data and len(d)==b['bytes'] and hashlib.sha256(d).hexdigest()==b['sha256'];sys.stdout.buffer.write(struct.pack('<I',len(d))+d)";
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
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut cursor = 0;
    let values = blobs
        .iter()
        .map(|blob| {
            assert!(cursor + 4 <= result.stdout.len());
            let len =
                u32::from_le_bytes(result.stdout[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            assert_eq!(len, word(&blob["bytes"]) as usize);
            assert!(cursor + len <= result.stdout.len());
            let data = result.stdout[cursor..cursor + len].to_vec();
            cursor += len;
            assert_eq!(blob["sha256"], format!("{:x}", Sha256::digest(&data)));
            data
        })
        .collect();
    assert_eq!(cursor, result.stdout.len());
    values
}
struct State {
    heap: Vec<u8>,
    stack: Vec<u8>,
    image: Vec<(u32, Vec<u8>)>,
    hp: u32,
    regs: [u32; 8],
}
fn states(rows: &[&Value]) -> Vec<State> {
    let mut blobs = vec![];
    for row in rows {
        blobs.push(&row["heap"]);
        blobs.push(&row["stack"]);
        assert_eq!(row["image"].as_array().unwrap().len(), 2);
        for (base, n) in IMAGE {
            let item = row["image"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| word(&v["base"]) == base)
                .unwrap();
            assert_eq!(word(&item["data"]["bytes"]) as usize, n);
            blobs.push(&item["data"]);
        }
    }
    let mut data = decode(&blobs).into_iter();
    rows.iter()
        .map(|row| {
            let heap = data.next().unwrap();
            let stack = data.next().unwrap();
            assert_eq!(heap.len(), 0x80000);
            assert_eq!(stack.len(), 0x1000);
            let image = IMAGE
                .into_iter()
                .map(|(base, n)| {
                    let bytes = data.next().unwrap();
                    assert_eq!(bytes.len(), n);
                    (base, bytes)
                })
                .collect();
            assert_eq!(row["registers"].as_object().unwrap().len(), 8);
            State {
                heap,
                stack,
                image,
                hp: word(&row["heap_ptr"]),
                regs: ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"]
                    .map(|n| word(&row["registers"][n])),
            }
        })
        .collect()
}
fn compare(m: &Space, state: &State, label: &str) {
    for (base, expected) in std::iter::once((HEAP, &state.heap))
        .chain(std::iter::once((0x7feff000, &state.stack)))
        .chain(state.image.iter().map(|(p, b)| (*p, b)))
    {
        let differences = expected
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let actual = m.r8(base + i as u32);
                (actual != *b).then_some((base + i as u32, actual, *b))
            })
            .take(8)
            .collect::<Vec<_>>();
        assert!(
            differences.is_empty(),
            "{label} memory{base:x} {differences:x?}"
        );
    }
    assert_eq!(m.heap_ptr(), state.hp, "{label} heapcursor");
    for ((r, name), expected) in regs().into_iter().zip(state.regs) {
        assert_eq!(reg(m, r), expected, "{label} {name}");
    }
    assert_eq!(stack::sp(m), state.regs[7], "{label} esp");
}
fn cstr(m: &Space, p: u32) -> String {
    let data = (0..4096)
        .map(|i| m.r8(p + i))
        .take_while(|b| *b != 0)
        .collect::<Vec<_>>();
    assert!(data.is_ascii());
    String::from_utf8(data).unwrap()
}
fn entries(root: &std::path::Path, pattern: &str) -> Vec<Value> {
    let (folder, mask) = pattern.rsplit_once('/').unwrap();
    assert_eq!(mask, "*");
    let stock = root.join("stock").canonicalize().unwrap();
    let mut path = stock.clone();
    for part in folder.split('/') {
        assert!(!matches!(part, "." | ".."));
        let choices = std::fs::read_dir(&path)
            .unwrap()
            .map(Result::unwrap)
            .filter(|e| e.file_name().to_str().unwrap().eq_ignore_ascii_case(part))
            .collect::<Vec<_>>();
        if choices.is_empty() {
            return vec![];
        }
        assert_eq!(choices.len(), 1);
        path = choices[0].path();
        assert!(path.canonicalize().unwrap().starts_with(&stock));
    }
    let mut values = vec![json!([".", true, 0]), json!(["..", true, 0])];
    for item in std::fs::read_dir(path).unwrap() {
        let item = item.unwrap();
        let name = item.file_name().into_string().unwrap();
        assert!(name.is_ascii());
        let meta = item.metadata().unwrap();
        values.push(json!([
            name,
            meta.is_dir(),
            if meta.is_dir() { 0 } else { meta.len() }
        ]));
    }
    values.sort_by_key(|v| v[0].as_str().unwrap().to_lowercase());
    values
}
struct Env<'a> {
    events: &'a [Value],
    states: &'a [State],
    index: usize,
    entries: Vec<Value>,
    handle: u32,
    cursor: usize,
    active: bool,
    pattern: String,
}
impl Env<'_> {
    fn check(&mut self, m: &Space, kind: &str, args: &[u32]) {
        let event = &self.events[self.index];
        assert_eq!(event["kind"], kind);
        assert_eq!(event["args"], json!(args));
        compare(
            m,
            &self.states[self.index],
            &format!("{kind}{}", self.index),
        );
        self.index += 1;
    }
    fn write_find(&self, m: &mut Space, destination: u32, index: usize) {
        let entry = &self.entries[index];
        let mut data = [0u8; 320];
        let flag = if entry[1].as_bool().unwrap() {
            16u32
        } else {
            32
        };
        data[..4].copy_from_slice(&flag.to_le_bytes());
        data[32..36].copy_from_slice(&word(&entry[2]).to_le_bytes());
        let name = entry[0].as_str().unwrap().as_bytes();
        assert!(name.len() < 260);
        data[44..44 + name.len()].copy_from_slice(name);
        for (i, b) in data.iter().enumerate() {
            m.w8(destination + i as u32, *b);
        }
    }
}
impl Allocator for Env<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.check(m, "allocate", &[n]);
        let p = m.alloc(n);
        assert!(m.heap_ptr() <= HEAP + 0x80000);
        Ok(p)
    }
    fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
        self.check(m, "reallocate", &[p, n]);
        let q = m.alloc(n);
        assert!(m.heap_ptr() <= HEAP + 0x80000);
        for i in 0..old.min(n) {
            m.w8(q + i, m.r8(p + i));
        }
        Ok(q)
    }
}
impl Environment for Env<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.check(m, "free", &[p]);
        Ok(0)
    }
    fn find_first(&mut self, m: &mut Space, pattern: u32, dst: u32) -> Result<u32, String> {
        self.check(m, "FindFirstFileA", &[pattern, dst]);
        assert_eq!(cstr(m, pattern).replace('\\', "/"), self.pattern);
        if self.entries.is_empty() {
            return Ok(u32::MAX);
        }
        assert!(!self.active);
        self.active = true;
        self.cursor = 1;
        self.write_find(m, dst, 0);
        Ok(self.handle)
    }
    fn find_next(&mut self, m: &mut Space, handle: u32, dst: u32) -> Result<u32, String> {
        self.check(m, "FindNextFileA", &[handle, dst]);
        assert!(self.active);
        assert_eq!(handle, self.handle);
        if self.cursor == self.entries.len() {
            return Ok(0);
        }
        self.write_find(m, dst, self.cursor);
        self.cursor += 1;
        Ok(1)
    }
    fn find_close(&mut self, m: &mut Space, handle: u32) -> Result<u32, String> {
        self.check(m, "FindClose", &[handle]);
        assert!(self.active);
        assert_eq!(handle, self.handle);
        self.active = false;
        Ok(1)
    }
}
#[test]
fn original_raw_find_stock_leaves_growth_existing_slots_and_full_slot_failure_preserve_complete_state(
) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(fixture_path) =
        crate::oracle_fixture::file(&root.join("re/out/boot/native-vm-find-original.json"))
    else {
        return;
    };
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(fixture_path).expect("run native_vm_find_probe.py"))
            .unwrap();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["cw"], 127);
    assert_eq!(fixture["unicorn"], "2.1.4");
    assert_eq!(
        fixture["exe_sha256"],
        format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        )
    );
    assert_eq!(fixture["producer_sources"].as_object().unwrap().len(), 2);
    for name in ["native_vm_find_probe.py", "oracle.py"] {
        assert_eq!(
            fixture["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let rows = fixture["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let cases = [
        ("ordinary", "system/scripts/lang/*", 0),
        ("growth", "parts/wheels/rims/scripts/*", 2),
        ("absent", "system/scripts/no_such_namespace/*", 0),
        ("slots_full", "system/scripts/lang/*", 32),
    ];
    for (index, row) in rows.iter().enumerate() {
        let (label, pattern, existing) = cases[index % 4];
        let fill = [0, 0xa5][index / 4];
        assert_eq!(
            row["input"],
            json!({"label":label,"pattern":pattern,"existing":existing,"fill":fill})
        );
        let actual_entries = entries(&root, pattern);
        let enumerations = row["enumerations"].as_array().unwrap();
        assert_eq!(enumerations.len(), 1);
        assert_eq!(enumerations[0]["pattern"], pattern);
        assert_eq!(enumerations[0]["entries"], json!(actual_entries));
        let handle = if actual_entries.is_empty() {
            u32::MAX
        } else {
            existing + 1
        };
        assert_eq!(enumerations[0]["handle"], handle);
        if label == "growth" {
            assert!(actual_entries.len() > 300);
        }
        let events = row["events"].as_array().unwrap();
        let raw_states = std::iter::once(&row["before"])
            .chain(events.iter().map(|e| &e["state"]))
            .chain(std::iter::once(&row["after"]))
            .collect::<Vec<_>>();
        let expected = states(&raw_states);
        let before = &expected[0];
        let mut m = Space::new();
        for (base, data) in std::iter::once((HEAP, &before.heap))
            .chain(std::iter::once((0x7feff000, &before.stack)))
            .chain(before.image.iter().map(|(p, b)| (*p, b)))
        {
            for (i, b) in data.iter().enumerate() {
                m.w8(base + i as u32, *b);
            }
        }
        m.set_heap_ptr(before.hp);
        for (r, name) in regs() {
            set(&mut m, r, word(&row["register_inputs"][name]));
        }
        let args = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(word)
            .collect::<Vec<_>>();
        assert_eq!(args.len(), 3);
        assert_eq!(cstr(&m, args[0]), pattern);
        let sp = ((0x7ff00000u32 - 0x100 - 16) & !15) - 4;
        stack::set_sp(&mut m, sp);
        m.w32(sp, 0x7fff0000);
        for (i, arg) in args.iter().enumerate() {
            m.w32(sp + 4 + i as u32 * 4, *arg);
        }
        m.w32(0x7ffdf004, 1);
        STORES.with(|s| s.borrow_mut().clear());
        set_test_store_observer(Some(record));
        let _observe = Observe;
        let mut env = Env {
            events,
            states: &expected[1..expected.len() - 1],
            index: 0,
            entries: actual_entries,
            handle,
            cursor: 0,
            active: false,
            pattern: pattern.into(),
        };
        let result = open(&mut m, args[0], args[1], args[2], &mut env).unwrap();
        assert_eq!(result, word(&row["result"]));
        assert_eq!(env.index, events.len());
        assert_eq!(env.active, label == "slots_full");
        assert_eq!(
            row["remaining_handles"],
            if env.active {
                json!([handle])
            } else {
                json!([])
            }
        );
        compare(
            &m,
            expected.last().unwrap(),
            &format!("row{index} {label} final"),
        );
        let writes = row["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (word(&v[0]), word(&v[1]), word(&v[2])))
            .collect::<Vec<_>>();
        STORES.with(|s| {
            let actual = s.borrow();
            assert_eq!(actual.len(), writes.len(), "row{index} rawwritecount");
            assert_eq!(
                actual.iter().zip(&writes).position(|(a, b)| a != b),
                None,
                "row{index} rawwriteorder"
            );
        });
    }
}
