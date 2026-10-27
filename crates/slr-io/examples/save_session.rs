//! Port side of the scripted save/config sessions (G9 / S8).
//!
//! Runs `re/oracle/save_sessions.json` through [`FileHost`] over a scratch
//! root and writes the same `results.json` / `files/` layout as the original
//! driver `re/oracle/save_session.py`, so `acceptance/validators.py` can
//! compare the two byte for byte and cross-load each side's files in the
//! other.
//!
//! ```text
//! cargo run -p slr-io --example save_session -- \
//!     --spec re/oracle/save_sessions.json --out DIR [--seed-from DIR] [--stock DIR]
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use slr_io::{FileHost, LoadRequest};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

/// What `fld dword ptr [..]` leaves in ST0 for the f32 `value`, as the
/// driver's `sign+exponent:mantissa` hex: exact widening, a signalling NaN
/// comes out quiet, f32 denormals are normalised.
fn ext80(value: f32) -> String {
    let bits = value.to_bits();
    let sign = ((bits >> 31) as u16) << 15;
    let exp = (bits >> 23) & 0xff;
    let frac = u64::from(bits & 0x7f_ffff);
    let (se, mant) = match (exp, frac) {
        (0, 0) => (sign, 0),
        (0, frac) => {
            let k = 63 - frac.leading_zeros();
            (sign | (k + 16383 - 149) as u16, frac << (63 - k))
        }
        (0xff, 0) => (sign | 0x7fff, 1 << 63),
        (0xff, frac) => (sign | 0x7fff, (1 << 63) | (frac << 40) | (1 << 62)),
        (exp, frac) => (sign | (exp + 16383 - 127) as u16, (1 << 63) | (frac << 40)),
    };
    format!("{se:04x}{mant:016x}")
}

fn sha256(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn files_under(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(root, &path, out);
        } else if let Ok(data) = fs::read(&path) {
            let name = path
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(name, data);
        }
    }
}

struct Args {
    spec: PathBuf,
    out: PathBuf,
    seed_from: Option<PathBuf>,
    stock: PathBuf,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut spec = None;
    let mut out = None;
    let mut seed_from = None;
    let mut stock = PathBuf::from("stock");
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--spec" => spec = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--seed-from" => seed_from = Some(PathBuf::from(value)),
            "--stock" => stock = PathBuf::from(value),
            other => panic!("unknown flag {other}"),
        }
    }
    Args {
        spec: spec.expect("--spec"),
        out: out.expect("--out"),
        seed_from,
        stock,
    }
}

fn load_seed(seed: &Value, args: &Args) -> Vec<u8> {
    match seed["from"].as_str().expect("seed.from") {
        "hex" => unhex(seed["hex"].as_str().expect("hex")),
        "stock" => fs::read(args.stock.join(seed["path"].as_str().expect("path")))
            .expect("stock seed readable"),
        "session" => {
            let base = args.seed_from.as_ref().unwrap_or(&args.out);
            fs::read(
                base.join("files")
                    .join(seed["session"].as_str().expect("session"))
                    .join(seed["name"].as_str().expect("name")),
            )
            .expect("session seed readable")
        }
        other => panic!("unknown seed source {other}"),
    }
}

fn u32_of(value: &Value) -> u32 {
    value.as_u64().expect("u32") as u32
}

fn tables(host: &FileHost, object: u32) -> Value {
    let Some(file) = host.native_file(object) else {
        return Value::Null;
    };
    json!({
        "resource_count": file.resource_count,
        "slot_count": file.slot_count,
        "resources": file.resources.iter().map(|r| json!([r.id, r.kind, r.path])).collect::<Vec<_>>(),
        "slots": file.slots.iter().map(|s| json!([s.id, s.kind, s.res_id])).collect::<Vec<_>>(),
    })
}

struct Session {
    host: FileHost,
    objects: BTreeMap<String, u32>,
    next_object: u32,
    next_word: u32,
    log_seen: usize,
}

impl Session {
    fn object(&mut self, label: &str, name: Option<&str>) -> u32 {
        if let Some(&object) = self.objects.get(label) {
            return object;
        }
        self.next_object += 1;
        let object = self.next_object;
        self.host.set_name(object, name.unwrap_or(label));
        self.objects.insert(label.to_owned(), object);
        object
    }

    fn run_op(&mut self, op: &Value) -> Value {
        let kind = op["op"].as_str().expect("op");
        let label = op["file"].as_str().expect("file");
        let object = if kind == "open" {
            self.objects.remove(label);
            self.object(label, op["name"].as_str())
        } else {
            self.object(label, op.get("name").and_then(Value::as_str))
        };
        self.host.take_loads();
        self.log_seen = self.host.log().len();
        let mut out = Map::new();
        out.insert("op".into(), json!(kind));
        out.insert("file".into(), json!(label));
        match kind {
            "open" => {
                let result = self.host.open(object, u32_of(&op["mode"]));
                out.insert("result".into(), json!(result));
            }
            "close" => self.host.close(object),
            "write_int" => {
                let result = self.host.write_int(object, u32_of(&op["value"]));
                out.insert("result".into(), json!(result));
            }
            "write_float" => {
                let result = self
                    .host
                    .write_float(object, f32::from_bits(u32_of(&op["bits"])));
                out.insert("result".into(), json!(result));
            }
            "write_string" => {
                self.next_word += 1;
                let word = self.next_word;
                self.host
                    .set_string(word, &unhex(op["hex"].as_str().expect("hex")));
                let result = self.host.write_string(object, word);
                out.insert("result".into(), json!(result));
            }
            "write_res_ref" => {
                let word = if op["ref"].is_null() {
                    0
                } else {
                    self.next_word += 1;
                    let word = self.next_word;
                    self.host.set_res_ref(
                        word,
                        u32_of(&op["ref"]["id"]),
                        u32_of(&op["ref"]["kind"]),
                    );
                    word
                };
                let result = slr_io::resid::write_res_ref(&mut self.host, object, word);
                out.insert("result".into(), json!(result));
            }
            "read_int" => {
                let result = self.host.read_int(object);
                out.insert("result".into(), json!(result));
            }
            "read_float" => {
                let result = ext80(self.host.read_float(object));
                out.insert("result".into(), json!(result));
            }
            "read_string" => {
                let word = self.host.read_string(object);
                let result = self.host.string(word).map(hex);
                out.insert("result".into(), json!(result));
            }
            "read_res_id" => {
                let result = slr_io::resid::read_res_id(&mut self.host, object);
                out.insert("result".into(), json!(result));
            }
            other => panic!("unknown op {other}"),
        }
        if kind == "open" {
            let handle = self.host.handle(object) != 0;
            out.insert("handle".into(), json!(handle));
            if handle {
                let file = self
                    .host
                    .native_file(object)
                    .expect("handle implies struct");
                out.insert(
                    "struct".into(),
                    json!({"mode": file.mode, "name": file.name(), "open_word": file.open_word}),
                );
                out.insert("tables".into(), tables(&self.host, object));
            }
        }
        let loads: Vec<Value> = self
            .host
            .take_loads()
            .into_iter()
            .map(|load| match load {
                LoadRequest::Companion { name } => json!(["load_companion", name]),
                LoadRequest::Blob { size, text } => json!(["load_blob", size, hex(&text)]),
            })
            .collect();
        out.insert("loads".into(), Value::Array(loads));
        out.insert(
            "log".into(),
            json!(self.host.log()[self.log_seen..].to_vec()),
        );
        Value::Object(out)
    }
}

fn main() {
    let args = parse_args();
    let spec_bytes = fs::read(&args.spec).expect("spec readable");
    let spec: Value = serde_json::from_slice(&spec_bytes).expect("spec json");
    fs::create_dir_all(&args.out).expect("out dir");
    let mut sessions = Map::new();
    for sess in spec["sessions"].as_array().expect("sessions") {
        let id = sess["id"].as_str().expect("id");
        let root = args.out.join("root").join(id);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("session root");
        let mut seeded = Vec::new();
        if let Some(seeds) = sess.get("seed").and_then(Value::as_object) {
            for (name, seed) in seeds {
                let dest = root.join(name);
                fs::create_dir_all(dest.parent().expect("parent")).expect("seed dir");
                fs::write(&dest, load_seed(seed, &args)).expect("seed write");
                seeded.push(name.clone());
            }
        }
        let mut host = FileHost::new(&root);
        for (kind, path) in spec["kinds"].as_object().expect("kinds") {
            host.set_kind_path(kind.parse().expect("kind"), path.as_str().expect("path"));
        }
        let mut session = Session {
            host,
            objects: BTreeMap::new(),
            next_object: 0,
            next_word: 0,
            log_seen: 0,
        };
        let ops: Vec<Value> = sess["ops"]
            .as_array()
            .expect("ops")
            .iter()
            .map(|op| session.run_op(op))
            .collect();
        let mut files = BTreeMap::new();
        files_under(&root, &root, &mut files);
        let mut hashes = Map::new();
        for (name, data) in &files {
            hashes.insert(name.clone(), json!(sha256(data)));
            if seeded.contains(name) {
                continue;
            }
            let dest = args.out.join("files").join(id).join(name);
            fs::create_dir_all(dest.parent().expect("parent")).expect("files dir");
            fs::write(dest, data).expect("file write");
        }
        sessions.insert(id.to_owned(), json!({"ops": ops, "files": hashes}));
    }
    let results = json!({
        "schema": 1,
        "implementation": "port",
        "spec_sha256": sha256(&spec_bytes),
        "sessions": sessions,
    });
    let mut text = serde_json::to_string_pretty(&results).expect("json");
    text.push('\n');
    fs::write(args.out.join("results.json"), text).expect("results write");
    println!(
        "{} sessions -> {}/results.json",
        sessions.len(),
        args.out.display()
    );
}
