//! Cross-thread interactive evidence. Input and outcomes are written before the
//! next operation; segments preserve history without an unbounded memory buffer.
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
struct Journal {
    root: PathBuf,
    run_id: String,
    file: File,
    segment: u64,
    bytes: u64,
    sequence: u64,
}
static JOURNAL: Mutex<Option<Journal>> = Mutex::new(None);
const SEGMENT_BYTES: u64 = if cfg!(test) { 1024 } else { 8 * 1024 * 1024 };
pub fn configure(root: &Path, run_id: &str) -> Result<(), String> {
    let mut slot = JOURNAL.lock().map_err(|e| e.to_string())?;
    if let Some(current) = slot.as_ref() {
        return if current.root == root {
            Ok(())
        } else {
            Err("session journal already configured elsewhere".into())
        };
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let file = open(root, 0)?;
    *slot = Some(Journal {
        root: root.into(),
        run_id: run_id.into(),
        file,
        segment: 0,
        bytes: 0,
        sequence: 0,
    });
    Ok(())
}
fn open(root: &Path, segment: u64) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(format!("session-{segment:05}.jsonl")))
        .map_err(|e| e.to_string())
}
pub fn enabled() -> bool {
    JOURNAL.lock().is_ok_and(|s| s.is_some())
}
pub fn root() -> Option<PathBuf> {
    JOURNAL.lock().ok()?.as_ref().map(|s| s.root.clone())
}
pub fn emit(kind: &str, data: Value) -> Result<u64, String> {
    let mut slot = JOURNAL.lock().map_err(|e| e.to_string())?;
    let Some(s) = slot.as_mut() else { return Ok(0) };
    append(s, kind, data, json!(slr_vm::diagnostics::current_context()))
}

fn append(s: &mut Journal, kind: &str, data: Value, context: Value) -> Result<u64, String> {
    s.sequence += 1;
    let value = json!({"schema_version":1,"run_id":s.run_id,"sequence":s.sequence,
        "wall_ns":crate::monotonic_ns(),"thread":format!("{:?}",std::thread::current().id()),
        "context":context,"kind":kind,"data":data});
    let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if s.bytes > 0 && s.bytes + bytes.len() as u64 > SEGMENT_BYTES {
        s.file.sync_all().map_err(|e| e.to_string())?;
        s.segment += 1;
        s.file = open(&s.root, s.segment)?;
        s.bytes = 0;
    }
    s.file
        .write_all(&bytes)
        .and_then(|()| s.file.sync_data())
        .map_err(|e| format!("SessionJournalBoundary: {e}"))?;
    s.bytes += bytes.len() as u64;
    Ok(s.sequence)
}

/// Record a panic and force the journal to disk, from inside a panic hook.
///
/// Never blocks and never panics: if the panicking thread holds the journal
/// lock the note is skipped (every earlier record was already synced by
/// [`emit`]), and a poisoned lock is still usable. The VM context is omitted
/// because reading it could itself panic mid-update.
pub fn panic_flush(message: &str) {
    let mut slot = match JOURNAL.try_lock() {
        Ok(slot) => slot,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    if let Some(s) = slot.as_mut() {
        let _ = append(s, "panic", json!({ "message": message }), Value::Null);
        let _ = s.file.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_input_records_are_durable_and_rotate_without_gaps() {
        let root = std::env::temp_dir().join(format!(
            "slr-journal-{}-{}",
            std::process::id(),
            crate::unix_ns()
        ));
        configure(&root, "journal-test").unwrap();
        std::thread::scope(|scope| {
            for worker in 0..4 {
                scope.spawn(move || {
                    for index in 0..50 {
                        emit(
                            "input-received",
                            json!({"worker":worker,"index":index,"padding":"x".repeat(100)}),
                        )
                        .unwrap();
                    }
                });
            }
        });
        let mut paths: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        assert!(paths.len() > 1);
        let mut rows = Vec::new();
        for path in paths {
            for line in std::fs::read_to_string(path).unwrap().lines() {
                rows.push(serde_json::from_str::<Value>(line).unwrap());
            }
        }
        assert_eq!(rows.len(), 200);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["sequence"], index + 1);
            assert_eq!(row["run_id"], "journal-test");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
