//! Differential checkpoints over genuine boot memory.
//!
//! `re/oracle/headless_snapshot.py` captures the original at a function entry
//! during the strict headless boot; `snapshot_replay.py` re-executes a graph
//! from it and records entry/exit page states of each traced function. A test
//! restores an entry state, runs the port, and compares every word the port
//! wrote and every word the original changed against the exit state.
use serde_json::Value;
use sha2::{Digest, Sha256};
use slr_physics::space::Space;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::path::{Component, Path};

const PAGE: u32 = 4096;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/snapshots")
}

pub struct Snapshot {
    pages: HashMap<u32, Vec<u8>>,
    checkpoints: Vec<Value>,
    blobs: Vec<u8>,
    /// Only declared scratch ranges; retained heap/data words are always compared.
    scratch_ranges: Vec<std::ops::Range<u32>>,
}

pub struct Entry {
    pub memory: Space,
    pub this: u32,
    pub stack: Vec<u32>,
    pub esp: u32,
    leave: Option<usize>,
}

fn number(value: &Value) -> Result<u32, String> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| "missing/invalid u32 fixture value".into())
}
fn validate_files(root: &Path, records: &Value, expected: &[String]) -> Result<(), String> {
    let records = records
        .as_object()
        .ok_or("missing producer/profile identities")?;
    if records.keys().collect::<BTreeSet<_>>() != expected.iter().collect::<BTreeSet<_>>() {
        return Err("producer/profile inventory mismatch".into());
    }
    for (path, hash) in records {
        if Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        {
            return Err("fixture identity path escapes repository".into());
        }
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
        if *hash != format!("{:x}", Sha256::digest(bytes)) {
            return Err(format!("stale producer/profile: {path}"));
        }
    }
    Ok(())
}

fn decode_pages(bytes: &[u8]) -> Result<HashMap<u32, Vec<u8>>, String> {
    let mut pages = HashMap::new();
    let (records, remainder) = bytes.as_chunks::<{ PAGE as usize + 4 }>();
    for record in records {
        let address = u32::from_le_bytes(record[..4].try_into().unwrap());
        if address % PAGE != 0 || pages.insert(address, record[4..].to_vec()).is_some() {
            return Err("unaligned or duplicate snapshot page".into());
        }
    }
    if !remainder.is_empty() {
        return Err("truncated snapshot page record".into());
    }
    Ok(pages)
}
fn decode_checkpoint(bytes: &[u8]) -> Result<(Value, Vec<u8>), String> {
    if bytes.len() < 4 {
        return Err("truncated checkpoint header".into());
    }
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let end = 4usize
        .checked_add(length)
        .filter(|&n| n <= bytes.len())
        .ok_or("checkpoint header beyond file")?;
    let header: Value = serde_json::from_slice(&bytes[4..end]).map_err(|e| e.to_string())?;
    let blobs = bytes[end..].to_vec();
    let count = number(&header["blobs"])? as usize;
    if blobs.len()
        != count
            .checked_mul(PAGE as usize)
            .ok_or("blob size overflow")?
    {
        return Err("truncated or trailing checkpoint blobs".into());
    }
    Ok((header, blobs))
}

impl Snapshot {
    /// Missing fixtures are explicit; a present but stale/malformed fixture
    /// fails even outside strict acceptance. Legacy captures must be recaptured.
    pub fn load(name: &str, scenario: &str) -> Option<Self> {
        let read = |suffix: &str| std::fs::read(root().join(format!("{name}{suffix}")));
        let inputs = (
            read(".pages"),
            read(".json"),
            read(&format!(".{scenario}.ckpt")),
            read(&format!(".{scenario}.json")),
        );
        match inputs {
            (Ok(pages), Ok(meta), Ok(ckpt), Ok(payload)) => Some(
                Self::parse(&pages, &meta, &ckpt, &payload).unwrap_or_else(|error| {
                    panic!("SLR_ORACLE_INVALID: {name}.{scenario}: {error}")
                }),
            ),
            _ => {
                eprintln!("SLR_ORACLE_SKIPPED: missing snapshot {name}.{scenario}");
                assert!(
                    std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
                    "required snapshot missing: {name}.{scenario}"
                );
                None
            }
        }
    }
    fn parse(
        pages_bytes: &[u8],
        metadata_bytes: &[u8],
        ckpt: &[u8],
        payload_bytes: &[u8],
    ) -> Result<Self, String> {
        let meta: Value = serde_json::from_slice(metadata_bytes).map_err(|e| e.to_string())?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        if meta["schema_version"] != 2 {
            return Err("legacy capture lacks provenance; recapture required".into());
        }
        let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        let identity = &meta["provenance"];
        let exe = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde";
        if identity["exe_sha256"] != exe
            || hash(
                &std::fs::read(source.join("reference/raw/StreetLegal_Redline.bak.exe"))
                    .map_err(|e| e.to_string())?,
            ) != exe
        {
            return Err("original executable identity mismatch".into());
        }
        let manifest = "671a67c0dfee0a546d1e873096d4df68a5791fe14acccd4da5ac99cd72adccce";
        if identity["stock"]["manifest_sha256"] != manifest
            || identity["stock"]["files"] != 7858
            || hash(&std::fs::read(source.join("stock/.slr-manifest")).map_err(|e| e.to_string())?)
                != manifest
        {
            return Err("stock manifest identity mismatch".into());
        }
        // Input bytes are verified, not accepted merely because a manifest exists.
        let stock =
            slr_io::stock::verify_stock(&source.join("stock")).map_err(|e| e.to_string())?;
        if !stock.extra.is_empty() {
            return Err("unexplained additional stock inputs".into());
        }
        let mut producers = vec![
            "re/oracle/headless_snapshot.py".to_string(),
            "re/oracle/oracle.py".to_string(),
            "acceptance/validators.py".to_string(),
        ];
        for file in std::fs::read_dir(source.join("re/headless")).map_err(|e| e.to_string())? {
            let path = file.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|v| v == "py") {
                producers.push(format!(
                    "re/headless/{}",
                    path.file_name()
                        .unwrap()
                        .to_str()
                        .ok_or("invalid producer name")?
                ));
            }
        }
        validate_files(&source, &identity["sources"], &producers)?;
        validate_files(&source, &identity["profiles"], &[
            "re/out/headless/hypothetical-cd-toc.json".into(),
            "re/out/headless/boot/graphics-profile.json".into(),
            "re/out/headless/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json".into(),
            "re/out/headless/boot/platform-profile.json".into()])?;
        if meta["pages_sha256"] != hash(pages_bytes) {
            return Err("snapshot pages digest mismatch".into());
        }
        let control = number(&meta["fpu_cw"])?;
        if control & 0xf00 != 0
            || number(&meta["fpu"]["control"])? != control
            || meta["fpu"]["registers"].as_array().map(|a| a.len()) != Some(8)
        {
            return Err("unsupported or missing captured x87 state".into());
        }
        let policy = &meta["comparison_policy"];
        if policy["id"] != "retained-native-memory-v1"
            || policy["retained_word_exclusions"] != serde_json::json!([])
            || policy["oracle_vm_scratch"] != serde_json::json!([0x30000000u32, 0x30100000u32])
            || policy["native_stack_bookkeeping"]
                != serde_json::json!([0x7ffdf000u32, 0x7ffe0000u32])
        {
            return Err("unsupported comparison policy or retained-word exclusion".into());
        }
        let mut scratch_ranges = vec![0x30000000..0x30100000, 0x7ffdf000..0x7ffe0000];
        let stacks = policy["thread_stack_ranges"]
            .as_array()
            .ok_or("missing actual stack ranges")?;
        for stack in stacks {
            let range = stack
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("malformed stack range")?;
            let low = number(&range[0])?;
            let high = number(&range[1])?;
            if !(low == 0x7fe00000 && high == 0x7ff00000
                || (0x40400000..0x42200000).contains(&low)
                    && (low - 0x40000000) % 0x200000 == 0
                    && high == low + 0x100000)
            {
                return Err("stack range outside declared original thread allocator".into());
            }
            scratch_ranges.push(low..high);
        }
        let pages = decode_pages(pages_bytes)?;
        if number(&meta["pages"])? as usize != pages.len() {
            return Err("snapshot page count mismatch".into());
        }
        let (header, blobs) = decode_checkpoint(ckpt)?;
        if header["schema_version"] != 2
            || header["snapshot_pages_sha256"] != meta["pages_sha256"]
            || header["snapshot_metadata_sha256"] != hash(metadata_bytes)
            || header["comparison_policy"] != *policy
            || header.get("stack_garbage").is_some()
            || header["truncated_functions"] != serde_json::json!([])
        {
            return Err(
                "stale/truncated checkpoint, legacy retained mask, or policy mismatch".into(),
            );
        }
        validate_files(
            &source,
            &header["producers"],
            &[
                "re/oracle/snapshot_replay.py".into(),
                "re/oracle/part_install_replay.py".into(),
                "re/oracle/oracle.py".into(),
            ],
        )?;
        if header["replay_payload_sha256"] != hash(payload_bytes) {
            return Err("stale replay payload digest".into());
        }
        let payload: Value = serde_json::from_slice(payload_bytes).map_err(|e| e.to_string())?;
        for field in [
            "schema_version",
            "snapshot_pages_sha256",
            "snapshot_metadata_sha256",
            "producers",
            "comparison_policy",
        ] {
            if payload[field] != header[field] {
                return Err(format!("replay payload binding mismatch: {field}"));
            }
        }
        if payload.get("stack_garbage").is_some()
            || !payload["events"].is_array()
            || !payload["diff"].is_array()
        {
            return Err("unsupported replay payload or retained-word mask".into());
        }
        let blob_count = number(&header["blobs"])? as usize;
        let checkpoints = header["checkpoints"]
            .as_array()
            .ok_or("missing checkpoint rows")?
            .clone();
        if checkpoints.is_empty() {
            return Err("empty checkpoint run".into());
        }
        for (index, row) in checkpoints.iter().enumerate() {
            let entries = row["pages"].as_array().ok_or("checkpoint missing pages")?;
            let mut seen = BTreeSet::new();
            for pair in entries {
                let pair = pair
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("malformed page/blob pair")?;
                let page = number(&pair[0])?;
                let blob = number(&pair[1])? as usize;
                if page % PAGE != 0 || blob >= blob_count || !seen.insert(page) {
                    return Err("bad checkpoint page/blob reference".into());
                }
            }
            number(&row["heap_ptr"])?;
            if row.get("enter").is_some() {
                if number(&row["index"])? as usize != index || number(&row["fpu_cw"])? != control {
                    return Err("checkpoint entry/control mismatch".into());
                }
                for r in ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"] {
                    number(&row["registers"][r])?;
                }
                if row["stack"].as_array().map(|a| a.len()) != Some(8) {
                    return Err("entry argument capture incomplete".into());
                }
            } else {
                let entry = number(&row["leave"])? as usize;
                if entry >= index || checkpoints[entry].get("enter").is_none() {
                    return Err("invalid checkpoint return pairing".into());
                }
                number(&row["eax"])?;
            }
        }
        Ok(Self {
            pages,
            checkpoints,
            blobs,
            scratch_ranges,
        })
    }

    fn blob(&self, index: u64) -> &[u8] {
        let start = index as usize * PAGE as usize;
        &self.blobs[start..start + PAGE as usize]
    }

    fn overlay(&self, memory: &mut Space, checkpoint: &Value) {
        for pair in checkpoint["pages"].as_array().unwrap() {
            let page = pair[0].as_u64().unwrap() as u32;
            let blob = self.blob(pair[1].as_u64().unwrap());
            for (i, byte) in blob.iter().enumerate() {
                memory.w8(page + i as u32, *byte);
            }
        }
    }

    fn base(&self) -> Space {
        let mut memory = Space::new();
        for (address, page) in &self.pages {
            for (i, byte) in page.iter().enumerate() {
                if *byte != 0 {
                    memory.w8(address + i as u32, *byte);
                }
            }
        }
        memory
    }

    /// Entry checkpoints for `function`, in original call order.
    pub fn entries(&self, function: u32) -> Vec<usize> {
        self.checkpoints
            .iter()
            .enumerate()
            .filter(|(_, c)| c["enter"].as_u64() == Some(function as u64))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn enter(&self, index: usize) -> Entry {
        let checkpoint = &self.checkpoints[index];
        let mut memory = self.base();
        self.overlay(&mut memory, checkpoint);
        memory.set_heap_ptr(checkpoint["heap_ptr"].as_u64().unwrap() as u32);
        let registers = &checkpoint["registers"];
        let esp = registers["esp"].as_u64().unwrap() as u32;
        crate::native_stack::set_sp(&mut memory, esp);
        for (name, reg) in [
            ("eax", crate::native_stack::Reg::Eax),
            ("ebx", crate::native_stack::Reg::Ebx),
            ("ecx", crate::native_stack::Reg::Ecx),
            ("edx", crate::native_stack::Reg::Edx),
            ("esi", crate::native_stack::Reg::Esi),
            ("edi", crate::native_stack::Reg::Edi),
            ("ebp", crate::native_stack::Reg::Ebp),
        ] {
            crate::native_stack::set_reg(
                &mut memory,
                reg,
                number(&registers[name]).expect("validated register"),
            );
        }
        memory.w32(0x7ffdf000, 0);
        memory.w32(0x7ffdf004, 1); // next ported entry consumes this captured CALL frame
        let leave = self
            .checkpoints
            .iter()
            .position(|c| c["leave"].as_u64() == Some(index as u64));
        Entry {
            memory,
            this: registers["ecx"].as_u64().unwrap() as u32,
            stack: checkpoint["stack"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect(),
            esp,
            leave,
        }
    }

    /// Exit eax for an entry.
    pub fn result(&self, entry: &Entry) -> u32 {
        self.checkpoints[entry.leave.expect("traced call returned")]["eax"]
            .as_u64()
            .unwrap() as u32
    }

    /// Compare the port's memory after `entry` with the original's exit state.
    /// Thread stacks are scratch (frames, spilled locals, reused argument
    /// slots) and excluded: an uninitialized stack word that reaches retained
    /// state still shows up where it lands. Returns `(address, original, port)`.
    pub fn compare(
        &self,
        entry: &Entry,
        port: &Space,
        written: &[(u32, u32)],
    ) -> Vec<(u32, u32, u32)> {
        let leave = &self.checkpoints[entry.leave.expect("traced call returned")];
        let mut expected = self.base();
        self.overlay(&mut expected, leave);
        let mut words = BTreeSet::new();
        for pair in leave["pages"].as_array().unwrap() {
            let page = pair[0].as_u64().unwrap() as u32;
            for i in (0..PAGE).step_by(4) {
                words.insert(page + i);
            }
        }
        for (address, _) in written {
            words.insert(address & !3);
        }
        let _ = entry.esp;
        let excluded = |a: u32| self.excluded(a);
        words
            .into_iter()
            .filter(|&a| !excluded(a))
            .filter_map(|a| {
                let (e, p) = (expected.r32(a), port.r32(a));
                (e != p).then_some((a, e, p))
            })
            .collect()
    }

    /// Compare against a final word diff relative to the snapshot (the
    /// outermost replayed call returns to the sentinel without an exit
    /// checkpoint).
    pub fn compare_final(
        &self,
        port: &Space,
        written: &[(u32, u32)],
        diff: &Value,
    ) -> Vec<(u32, u32, u32)> {
        let mut expected = self.base();
        let mut words = BTreeSet::new();
        for row in diff.as_array().unwrap() {
            let address = row[0].as_u64().unwrap() as u32;
            expected.w32(address, row[2].as_u64().unwrap() as u32);
            words.insert(address);
        }
        for (address, _) in written {
            words.insert(address & !3);
        }
        let excluded = |a: u32| self.excluded(a);
        words
            .into_iter()
            .filter(|&a| !excluded(a))
            .filter_map(|a| {
                let (e, p) = (expected.r32(a), port.r32(a));
                (e != p).then_some((a, e, p))
            })
            .collect()
    }

    /// Stack/VM-call scratch and the port's bookkeeping page are declared
    /// in the producer profile. No retained data/heap field is masked.
    fn excluded(&self, a: u32) -> bool {
        self.scratch_ranges.iter().any(|range| range.contains(&a))
    }

    pub fn expected_heap_ptr(&self, entry: &Entry) -> u32 {
        self.checkpoints[entry.leave.expect("traced call returned")]["heap_ptr"]
            .as_u64()
            .unwrap() as u32
    }
}

/// Format the first mismatches for an assertion message.
pub fn report(mismatches: &[(u32, u32, u32)]) -> String {
    mismatches
        .iter()
        .take(24)
        .map(|(a, e, p)| format!("{a:#010x}: original {e:#010x} port {p:#010x}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_duplicate_and_unaligned_pages_are_rejected() {
        let record = [0u8; 4100];
        assert!(decode_pages(&record[..4099]).is_err());
        assert!(decode_pages(&[record.as_slice(), record.as_slice()].concat()).is_err());
        let mut unaligned = record;
        unaligned[0] = 1;
        assert!(decode_pages(&unaligned).is_err());
    }
    #[test]
    fn checkpoint_lengths_and_blob_inventory_fail_closed() {
        assert!(decode_checkpoint(&[]).is_err());
        assert!(decode_checkpoint(&u32::MAX.to_le_bytes()).is_err());
        let header = br#"{"blobs":1}"#;
        let mut bytes = (header.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(header);
        assert!(decode_checkpoint(&bytes).is_err());
        bytes.extend_from_slice(&[0; 4096]);
        assert!(decode_checkpoint(&bytes).is_ok());
        bytes.push(0);
        assert!(decode_checkpoint(&bytes).is_err());
    }
    #[test]
    fn legacy_captures_cannot_be_relabelled_as_verified() {
        let error = Snapshot::parse(&[], br#"{"fpu_cw":63}"#, &[], &[])
            .err()
            .unwrap();
        assert!(error.contains("legacy capture lacks provenance"));
    }
    #[test]
    fn retained_fields_are_compared_even_when_their_bytes_came_from_stack() {
        let snapshot = Snapshot {
            pages: HashMap::new(),
            checkpoints: vec![],
            blobs: vec![],
            scratch_ranges: std::iter::once(0x7fe00000..0x7ff00000).collect(),
        };
        let mut memory = Space::new();
        memory.w32(0x2000000c, 9);
        memory.w32(0x7fe0000c, 9);
        let diff = serde_json::json!([[0x2000000cu32, 0, 1], [0x7fe0000cu32, 0, 1]]);
        assert_eq!(
            snapshot.compare_final(&memory, &[(0x2000000c, 9), (0x7fe0000c, 9)], &diff),
            vec![(0x2000000c, 1, 9)]
        );
    }
}
