//! Original54d290/5553b0 file-manager publication and64request slot identities.
//! The worker owns subsequent I/O and callback dispatch.
use slr_physics::space::Space;
pub const REQUESTS: u32 = 0x76aa30;
pub const STRIDE: u32 = 0x11c;
pub trait Signal {
    fn signal(&mut self, m: &mut Space, semaphore: i32) -> Result<(), String>;
}
/// Memory initialization prefix of554e10. Active76f980 is published only when
/// the actual semaphore/thread startup collaborator has succeeded.
pub fn initialize_records(m: &mut Space) {
    for p in (REQUESTS..REQUESTS + 64 * STRIDE).step_by(4) {
        m.w32(p, 0);
    }
    m.w32(0x76f134, 0);
    for i in 0..8 {
        m.w32(0x76f238 + i * 0x108, u32::MAX);
    }
}
/// Original54d290 normalizes separators before5553b0 copies the request name.
/// Success is the live slot index, including zero. It does not imply I/O done.
pub fn request(
    m: &mut Space,
    path: &[u8],
    destination: u32,
    callback: u32,
    arg: u32,
    priority: u32,
    signal: &mut dyn Signal,
) -> Result<i32, String> {
    if m.r8(0x76f980) == 0 {
        return Ok(-1);
    }
    let free = (0..64).find(|i| m.r32(REQUESTS + i * STRIDE) == 0);
    if let Some(index) = free {
        let p = REQUESTS + index * STRIDE;
        m.w32(p + 4, destination);
        m.w32(p + 0x108, 0);
        m.w32(p + 0x10c, 0);
        m.w32(p + 0x110, arg);
        m.w32(p + 0x114, priority);
        m.w32(p + 0x118, callback);
        let end = path
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(path.len())
            .min(255);
        for (i, byte) in path[..end].iter().enumerate() {
            m.w8(p + 8 + i as u32, if *byte == b'/' { b'\\' } else { *byte });
        }
        m.w8(p + 8 + end as u32, 0);
        m.w32(p, 1);
    }
    signal.signal(m, m.r32(0x61ac3c) as i32)?;
    if let Some(index) = free {
        Ok(index as i32)
    } else {
        m.w32(0x76f998, m.r32(0x76f998).wrapping_add(1));
        Ok(-5)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Signals(Vec<i32>);
    impl Signal for Signals {
        fn signal(&mut self, _: &mut Space, semaphore: i32) -> Result<(), String> {
            self.0.push(semaphore);
            Ok(())
        }
    }
    #[test]
    fn original_file_request_slot_identity_publication_and_saturation_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-file-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_file_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for (i, v) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(REQUESTS + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            m.w32(0x76f980, row["active"].as_u64().unwrap() as u32);
            m.w32(0x61ac3c, 123);
            m.w32(0x76f998, 7);
            let mut signals = Signals(vec![]);
            let result = request(
                &mut m,
                row["text"].as_str().unwrap().as_bytes(),
                0x30001000,
                0x505c00,
                0x20001234,
                8,
                &mut signals,
            )
            .unwrap();
            assert_eq!(result as u32, row["result"].as_u64().unwrap() as u32);
            assert_eq!(serde_json::json!(signals.0), row["calls"]);
            assert_eq!(m.r32(0x76f998), row["fullcount"].as_u64().unwrap() as u32);
            for (i, v) in row["output"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(REQUESTS + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "active{} fill{} free{} word{i}",
                    row["active"],
                    row["fill"],
                    row["free"]
                );
            }
        }
    }
}

/// File-system and callback seams called by original554ea0. Handles below
/// 0x10000 name loose files; larger handles name the original packed-file layer.
/// Negative open results retain original semantics (-2 is a retry).
pub trait WorkerHost {
    fn open(&mut self, path: &[u8]) -> Result<i32, String>;
    fn close(&mut self, handle: i32) -> Result<(), String>;
    fn seek(&mut self, handle: i32, offset: u32) -> Result<(), String>;
    fn length(&mut self, handle: i32) -> Result<u32, String>;
    fn allocate(&mut self, m: &mut Space, length: u32) -> Result<u32, String>;
    fn read(
        &mut self,
        m: &mut Space,
        handle: i32,
        destination: u32,
        length: u32,
    ) -> Result<i32, String>;
    fn complete(
        &mut self,
        m: &mut Space,
        callback: u32,
        argument: u32,
        result: i32,
        destination: u32,
        length: u32,
    ) -> Result<(), String>;
}
/// Local selection cursor from554ea0; it is not reset between wakeups.
#[derive(Default)]
pub struct Worker {
    previous: Option<u32>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Retried(u32),
    Completed(u32),
}
fn name(m: &Space, p: u32) -> Vec<u8> {
    (0..256)
        .map(|i| m.r8(p + i))
        .take_while(|b| *b != 0)
        .collect()
}
impl Worker {
    /// Execute one complete request. A host owns serialization with publishers
    /// and the wakeup loop; this routine never claims a pending file is ready.
    pub fn step(&mut self, m: &mut Space, host: &mut dyn WorkerHost) -> Result<Step, String> {
        let mut selected = None;
        for index in 0..64 {
            let p = REQUESTS + index * STRIDE;
            if m.r32(p) != 1 {
                continue;
            }
            if let Some(old) = selected {
                let priority = m.r32(p + 0x114) as i32;
                let old_priority = m.r32(REQUESTS + old * STRIDE + 0x114) as i32;
                let previous = self.previous.map_or(-1, |p| p as i32);
                if priority <= old_priority
                    && (previous < old as i32
                        || priority != old_priority
                        || index as i32 <= previous)
                {
                    continue;
                }
            }
            selected = Some(index);
        }
        let Some(index) = selected else {
            self.previous = None;
            return Ok(Step::Idle);
        };
        self.previous = Some(index);
        let p = REQUESTS + index * STRIDE;
        let mut destination = m.r32(p + 4);
        let offset = m.r32(p + 0x108);
        let mut length = m.r32(p + 0x10c);
        let argument = m.r32(p + 0x110);
        let callback = m.r32(p + 0x118);
        let path = name(m, p + 8);
        m.w32(p, 2);
        m.w32(0x76f98c, m.r32(0x76f98c).wrapping_add(1));
        let cached = (0..8).find(|i| {
            let cache = 0x76f138 + i * 0x108;
            m.r32(cache + 0x100) as i32 >= 0 && name(m, cache).eq_ignore_ascii_case(&path)
        });
        let handle = if let Some(i) = cached {
            let cache = 0x76f138 + i * 0x108;
            m.w32(cache + 0x104, m.r32(0x76f134));
            m.w32(0x76f134, m.r32(0x76f134).wrapping_add(1));
            m.r32(cache + 0x100) as i32
        } else {
            let mut oldest = 0;
            for i in 0..8 {
                let cache = 0x76f138 + i * 0x108;
                if (m.r32(cache + 0x100) as i32) < 0 {
                    oldest = i;
                    break;
                }
                if (m.r32(cache + 0x104) as i32) < (m.r32(0x76f23c + oldest * 0x108) as i32) {
                    oldest = i;
                }
            }
            let cache = 0x76f138 + oldest * 0x108;
            let old = m.r32(cache + 0x100) as i32;
            if old >= 0 {
                host.close(old)?;
                m.w32(cache + 0x100, u32::MAX);
            }
            let handle = host.open(&path)?;
            if handle >= 0 {
                m.w32(cache + 0x100, handle as u32);
                m.w32(cache + 0x104, m.r32(0x76f134));
                m.w32(0x76f134, m.r32(0x76f134).wrapping_add(1));
                for (i, b) in path.iter().chain(std::iter::once(&0)).enumerate() {
                    m.w8(cache + i as u32, *b);
                }
            }
            handle
        };
        if handle == -2 {
            m.w32(p, 1);
            return Ok(Step::Retried(index));
        }
        let result = if handle < 0 {
            -3
        } else {
            host.seek(handle, offset)?;
            if length == 0 {
                length = host.length(handle)?.wrapping_sub(offset);
            }
            if destination == 0 {
                destination = host.allocate(m, length)?;
            }
            if destination == 0 {
                -4
            } else {
                let read = host.read(m, handle, destination, length)?;
                m.w32(0x76f994, m.r32(0x76f994).wrapping_add(read as u32));
                if read as u32 == length {
                    0
                } else {
                    -1
                }
            }
        };
        if callback != 0 {
            host.complete(m, callback, argument, result, destination, length)?;
        }
        m.w32(p, 0);
        Ok(Step::Completed(index))
    }
}
#[cfg(test)]
mod worker_tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    struct Files {
        handles: HashMap<i32, (String, u32)>,
        next: i32,
        calls: Vec<Value>,
        completions: Vec<Value>,
        retry: bool,
        requeue: bool,
    }
    impl WorkerHost for Files {
        fn open(&mut self, path: &[u8]) -> Result<i32, String> {
            let path = String::from_utf8(path.to_vec()).unwrap();
            self.calls.push(json!(["open", path]));
            if path == "missing" {
                return Ok(-1);
            }
            if path == "retry" && !self.retry {
                self.retry = true;
                return Ok(-2);
            }
            let h = self.next;
            self.next += 1;
            self.handles.insert(h, (path, 0));
            Ok(h)
        }
        fn close(&mut self, h: i32) -> Result<(), String> {
            self.calls.push(json!(["close", h]));
            Ok(())
        }
        fn seek(&mut self, h: i32, o: u32) -> Result<(), String> {
            self.handles.get_mut(&h).unwrap().1 = o;
            self.calls.push(json!(["seek", h, o]));
            Ok(())
        }
        fn length(&mut self, h: i32) -> Result<u32, String> {
            self.calls.push(json!(["length", h]));
            Ok(if self.handles[&h].0 == "empty" { 0 } else { 12 })
        }
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            Ok(if n < 100 { m.malloc(n) } else { 0 })
        }
        fn read(&mut self, m: &mut Space, h: i32, d: u32, n: u32) -> Result<i32, String> {
            let (path, offset) = &self.handles[&h];
            let mut count = n.min(12 - offset);
            if path == "short" {
                count = count.saturating_sub(1);
            }
            for i in 0..count {
                m.w8(d + i, (offset + i) as u8);
            }
            self.calls.push(json!(["read", h, d, n, count]));
            Ok(count as i32)
        }
        fn complete(
            &mut self,
            m: &mut Space,
            cb: u32,
            arg: u32,
            result: i32,
            d: u32,
            n: u32,
        ) -> Result<(), String> {
            assert_eq!(cb, 0x30001000);
            let bytes = if d != 0 && n < 100 {
                (0..n).map(|i| m.r8(d + i)).collect::<Vec<_>>()
            } else {
                vec![]
            };
            self.completions.push(json!({"args":[arg,result as u32,d,n],"state":m.r32(REQUESTS+arg*STRIDE),"bytes":bytes}));
            if self.requeue {
                self.requeue = false;
                for slot in [0, 2] {
                    let p = REQUESTS + slot * STRIDE;
                    m.w32(p, 1);
                    m.w32(p + 0x110, slot);
                    m.w32(p + 0x118, 0x30001000);
                    for (i, b) in b"same\0".iter().enumerate() {
                        m.w8(p + 8 + i as u32, *b);
                    }
                }
            }
            Ok(())
        }
    }
    #[test]
    fn original_worker_priority_cache_ranges_failures_and_callback_order_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-file-worker-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let data: Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/native_file_worker_probe.py first"),
        )
        .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            initialize_records(&mut m);
            for (i, v) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(REQUESTS + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut files = Files {
                handles: HashMap::new(),
                next: 7,
                calls: vec![],
                completions: vec![],
                retry: false,
                requeue: row["requeue"].as_bool().unwrap(),
            };
            let mut worker = Worker::default();
            let mut steps = 0;
            while worker.step(&mut m, &mut files).unwrap() != Step::Idle {
                steps += 1;
                assert!(steps < 100);
            }
            assert_eq!(json!(files.calls), row["calls"], "{} calls", row["name"]);
            assert_eq!(
                json!(files.completions),
                row["completions"],
                "{} completions",
                row["name"]
            );
            for (field, base) in [("requests", REQUESTS), ("cache", 0x76f138)] {
                for (i, v) in row[field].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(base + i as u32 * 4),
                        v.as_u64().unwrap() as u32,
                        "{} {field}[{i}]",
                        row["name"]
                    );
                }
            }
            for (field, address) in [
                ("serial", 0x76f134),
                ("processed", 0x76f98c),
                ("read_bytes", 0x76f994),
            ] {
                assert_eq!(
                    m.r32(address),
                    row[field].as_u64().unwrap() as u32,
                    "{} {field}",
                    row["name"]
                );
            }
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
}

/// A serialized host service for the original file worker. The boot scheduler
/// explicitly calls `pump`; publishing a request only signals its pending wake.
/// This does not emulate OS thread scheduling or consume captured outputs.
struct Runtime {
    worker: Worker,
    files: RuntimeFiles,
    wake: bool,
}
#[derive(Default)]
struct RuntimeFiles {
    files: std::collections::HashMap<i32, std::fs::File>,
}
thread_local! { static RUNTIME: std::cell::RefCell<Option<Runtime>> = const { std::cell::RefCell::new(None) }; }
/// Install the actual read/callback service before publishing manager activity.
/// The embedding chooses its scheduling boundary separately.
pub fn initialize_runtime(m: &mut Space) {
    initialize_records(m);
    RUNTIME.with(|state| {
        *state.borrow_mut() = Some(Runtime {
            worker: Worker::default(),
            files: RuntimeFiles::default(),
            wake: false,
        })
    });
    m.w8(0x76f980, 1);
}
struct Wake;
impl Signal for Wake {
    fn signal(&mut self, _m: &mut Space, _semaphore: i32) -> Result<(), String> {
        RUNTIME.with(|state| {
            let mut state = state.borrow_mut();
            let runtime = state
                .as_mut()
                .ok_or("NativeFileBoundary: worker service is not initialized")?;
            runtime.wake = true;
            Ok(())
        })
    }
}
pub fn request_shape(m: &mut Space, path: &str, callback: u32, shape: u32) -> Result<i32, String> {
    if !RUNTIME.with(|state| state.borrow().is_some()) {
        return Err("NativeFileBoundary: worker service is not initialized".into());
    }
    request(m, path.as_bytes(), 0, callback, shape, 0, &mut Wake)
}
/// Original54d300/555470 range request: allocation precedes publication and
/// success returns the requested length, not the queue slot index.
pub fn request_range(
    m: &mut Space,
    path: &str,
    offset: u32,
    length: u32,
    callback: u32,
    arg: u32,
    priority: u32,
) -> Result<i32, String> {
    if m.r8(0x76f980) == 0 {
        return Ok(-1);
    }
    if !RUNTIME.with(|state| state.borrow().is_some()) {
        return Err("NativeFileBoundary: worker service is not initialized".into());
    }
    let free = (0..64).find(|i| m.r32(REQUESTS + i * STRIDE) == 0);
    if let Some(i) = free {
        let data = if (length as i32) > 0 {
            m.malloc(length)
        } else {
            0
        };
        let p = REQUESTS + i * STRIDE;
        m.w32(p + 4, data);
        m.w32(p + 0x108, offset);
        m.w32(p + 0x10c, length);
        m.w32(p + 0x110, arg);
        m.w32(p + 0x114, priority);
        m.w32(p + 0x118, callback);
        let bytes = path.as_bytes();
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(bytes.len())
            .min(255);
        for (i, b) in bytes[..end].iter().enumerate() {
            m.w8(p + 8 + i as u32, if *b == b'/' { b'\\' } else { *b });
        }
        m.w8(p + 8 + end as u32, 0);
        m.w32(p, 1);
    }
    Wake.signal(m, m.r32(0x61ac3c) as i32)?;
    if free.is_some() {
        Ok(length as i32)
    } else {
        m.w32(0x76f998, m.r32(0x76f998).wrapping_add(1));
        Ok(-5)
    }
}
/// One original worker request quantum. Repeated calls drain work; Idle clears
/// the pending wake. No decode/GPU readiness is synthesized by file completion.
pub fn pump(m: &mut Space) -> Result<Step, String> {
    RUNTIME.with(|state| {
        let mut state = state.borrow_mut();
        let runtime = state
            .as_mut()
            .ok_or("NativeFileBoundary: worker service is not initialized")?;
        if !runtime.wake {
            return Ok(Step::Idle);
        }
        let result = runtime.worker.step(m, &mut runtime.files)?;
        if result == Step::Idle {
            runtime.wake = false;
        }
        Ok(result)
    })
}
impl WorkerHost for RuntimeFiles {
    fn open(&mut self, path: &[u8]) -> Result<i32, String> {
        let path =
            std::str::from_utf8(path).map_err(|_| "NativeFileBoundary: non-UTF8 stock path")?;
        let relative = path.replace('\\', "/");
        let relative = std::path::Path::new(&relative);
        if relative.is_absolute()
            || relative.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err("NativeFileBoundary: file path escapes stock".into());
        }
        let opened = crate::RES.with(|resources| {
            let root = resources.borrow().root.canonicalize()?;
            let path = root.join(relative).canonicalize()?;
            if !path.starts_with(&root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "stock path escape",
                ));
            }
            std::fs::File::open(path)
        });
        let file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(-1),
            Err(error) => return Err(format!("NativeFileBoundary: stock open {path:?}: {error}")),
        };
        let handle = (0..0x10000)
            .find(|h| !self.files.contains_key(h))
            .ok_or("NativeFileBoundary: loose file handles exhausted")?;
        self.files.insert(handle, file);
        Ok(handle)
    }
    fn close(&mut self, handle: i32) -> Result<(), String> {
        self.files
            .remove(&handle)
            .ok_or("NativeFileBoundary: close unknown file")?;
        Ok(())
    }
    fn seek(&mut self, handle: i32, offset: u32) -> Result<(), String> {
        use std::io::{Seek, SeekFrom};
        self.files
            .get_mut(&handle)
            .ok_or("NativeFileBoundary: seek unknown file")?
            .seek(SeekFrom::Start(offset.into()))
            .map_err(|e| format!("NativeFileBoundary: seek: {e}"))?;
        Ok(())
    }
    fn length(&mut self, handle: i32) -> Result<u32, String> {
        let length = self
            .files
            .get(&handle)
            .ok_or("NativeFileBoundary: length unknown file")?
            .metadata()
            .map_err(|e| format!("NativeFileBoundary: length: {e}"))?
            .len();
        u32::try_from(length)
            .map_err(|_| "NativeFileBoundary: file larger than 32-bit length".into())
    }
    fn allocate(&mut self, m: &mut Space, length: u32) -> Result<u32, String> {
        Ok(if (length as i32) < 1 {
            0
        } else {
            m.malloc(length)
        })
    }
    fn read(
        &mut self,
        m: &mut Space,
        handle: i32,
        destination: u32,
        length: u32,
    ) -> Result<i32, String> {
        use std::io::Read;
        let file = self
            .files
            .get_mut(&handle)
            .ok_or("NativeFileBoundary: read unknown file")?;
        let mut bytes = vec![0; length as usize];
        let count = file
            .read(&mut bytes)
            .map_err(|e| format!("NativeFileBoundary: read: {e}"))?;
        for (i, b) in bytes[..count].iter().enumerate() {
            m.w8(destination + i as u32, *b);
        }
        Ok(count as i32)
    }
    fn complete(
        &mut self,
        m: &mut Space,
        callback: u32,
        argument: u32,
        result: i32,
        data: u32,
        length: u32,
    ) -> Result<(), String> {
        match callback {
            0x505c00 => crate::native_shape::complete_file(m, argument, result, data, length),
            0x544540 => {
                crate::native_resource_manager::complete(m, argument, result, data);
                Ok(())
            }
            _ => Err(format!(
                "NativeFileBoundary: callback {callback:08x} is not integrated"
            )),
        }
    }
}
#[cfg(test)]
mod runtime_tests {
    use super::*;
    struct ShapeHost;
    impl crate::native_shape::Host for ShapeHost {
        fn request_file(
            &mut self,
            m: &mut Space,
            path: &str,
            callback: u32,
            shape: u32,
        ) -> Result<i32, String> {
            request_shape(m, path, callback, shape)
        }
    }
    #[test]
    fn actual_file_bytes_complete_only_when_worker_is_pumped() {
        let root = std::env::temp_dir().join(format!(
            "slr-file-worker-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(root.join("mesh")).unwrap();
        let bytes = b"SCX\0\x04\0\0\0\0\0\0\0actual-file-payload";
        std::fs::write(root.join("mesh/test.scx"), bytes).unwrap();
        let previous = crate::RES
            .with(|resources| std::mem::replace(&mut resources.borrow_mut().root, root.clone()));
        let mut m = Space::new();
        initialize_runtime(&mut m);
        crate::native_shape::initialize_pending_queue(&mut m);
        crate::native_shape::initialize_retry_queue(&mut m);
        crate::native_shape::initialize_ready_queue(&mut m);
        let shape = crate::native_shape::request_async(
            &mut m,
            3,
            "mesh/test.scx",
            0,
            0x1234,
            0,
            &mut ShapeHost,
        )
        .unwrap();
        assert_eq!(m.r32(shape + 0x20), 1);
        assert_eq!(m.r32(shape + 0x28), 0);
        assert_eq!(m.r32(REQUESTS), 1);
        assert_eq!(m.r32(0x65c204), 1);
        assert_eq!(m.r32(0x65c208), 0);
        assert_eq!(pump(&mut m).unwrap(), Step::Completed(0));
        assert_eq!(m.r32(REQUESTS), 0);
        assert_eq!(m.r32(shape + 0x20), 2);
        assert_eq!(m.r32(0x65c204), 0);
        assert_eq!(m.r32(0x65c208), 1);
        assert_eq!(m.r32(0x65c1a0), shape);
        assert_eq!(m.r32(shape + 0x2c), bytes.len() as u32);
        let data = m.r32(shape + 0x28);
        assert_eq!(
            (0..bytes.len())
                .map(|i| m.r8(data + i as u32))
                .collect::<Vec<_>>(),
            bytes
        );
        assert_eq!(pump(&mut m).unwrap(), Step::Idle);
        let record = m.alloc(0x3c);
        m.w32(record + 0x28, 0x80000000);
        m.w32(record + 0x2c, 1);
        assert_eq!(
            request_range(&mut m, "mesh/test.scx", 4, 4, 0x544540, record, 0).unwrap(),
            4
        );
        assert_eq!(m.r32(record + 0x2c), 1);
        assert_eq!(m.r32(record + 0x30), 0);
        assert_eq!(pump(&mut m).unwrap(), Step::Completed(0));
        assert_eq!(m.r32(record + 0x2c), 0);
        assert_eq!(m.r32(record + 0x28), 0x80000001);
        let data = m.r32(record + 0x30);
        assert_eq!(
            (0..4).map(|i| m.r8(data + i)).collect::<Vec<_>>(),
            bytes[4..8]
        );
        assert_eq!(pump(&mut m).unwrap(), Step::Idle);
        RUNTIME.with(|state| *state.borrow_mut() = None);
        crate::RES.with(|resources| resources.borrow_mut().root = previous);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[cfg(test)]
mod range_tests {
    use super::*;
    #[test]
    fn original_range_request_allocation_publication_and_length_result_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-file-range-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("run native_file_range_probe.py"))
                .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            initialize_runtime(&mut m);
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            for (i, v) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(REQUESTS + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            m.w32(0x76f980, row["active"].as_u64().unwrap() as u32);
            let result = request_range(
                &mut m,
                row["text"].as_str().unwrap(),
                1234,
                row["length"].as_u64().unwrap() as u32,
                0x544540,
                0x20010000,
                7,
            )
            .unwrap();
            assert_eq!(result as u32, row["result"].as_u64().unwrap() as u32);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            assert_eq!(m.r32(0x76f998), row["fullcount"].as_u64().unwrap() as u32);
            assert_eq!(
                RUNTIME.with(|r| r.borrow().as_ref().unwrap().wake),
                !row["calls"].as_array().unwrap().is_empty()
            );
            for (i, v) in row["output"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(REQUESTS + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "word{i}"
                );
            }
        }
        RUNTIME.with(|r| *r.borrow_mut() = None);
    }
}
