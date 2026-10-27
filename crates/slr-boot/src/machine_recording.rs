//! Durable opt-in transport for the causal VM stream. Acknowledgement follows
//! successful serialization, write, flush and fsync. An I/O failure is latched;
//! retry cannot append a duplicate prefix to a partially written stream.
use slr_vm::machine::Trace;
use std::{cell::RefCell, fs::File, io::Write, path::Path};
thread_local! {static RECORDING:RefCell<Option<Drain<File>>>=const{RefCell::new(None)};}
struct Drain<W> {
    writer: W,
    failure: Option<String>,
}
impl<W: Write> Drain<W> {
    fn drain(
        &mut self,
        trace: &Trace,
        mut durable: impl FnMut(&mut W) -> std::io::Result<()>,
    ) -> Result<(), String> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result: Result<(), String> = (|| loop {
            let entries = trace.pending(4096);
            if entries.is_empty() {
                return Ok(());
            }
            let through = entries.last().unwrap().sequence;
            let mut bytes = Vec::new();
            for entry in entries {
                serde_json::to_writer(&mut bytes, &entry)
                    .map_err(|e| format!("MachineRecordingBoundary: encode: {e}"))?;
                bytes.push(b'\n');
            }
            self.writer
                .write_all(&bytes)
                .and_then(|_| self.writer.flush())
                .and_then(|_| durable(&mut self.writer))
                .map_err(|e| format!("MachineRecordingBoundary: write/flush/sync: {e}"))?;
            trace
                .acknowledge(through)
                .map_err(|e| format!("MachineRecordingBoundary: acknowledge: {e}"))?;
        })();
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }
}
pub fn start(vm: &mut slr_vm::host::VmHost, output: &Path, run_id: &str) -> Result<(), String> {
    vm.start_machine_recording()
        .map_err(|e| format!("MachineRecordingBoundary: {e}"))?;
    let path = output.join("machine-events.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("MachineRecordingBoundary: create {}: {e}", path.display()))?;
    serde_json::to_writer(&mut file,&serde_json::json!({"schema_version":1,"kind":"machine-start","run_id":run_id,"identity_scope":"run","value_encoding":"serde TypeTag variants and u32 payload bits"})).map_err(|e|format!("MachineRecordingBoundary: header: {e}"))?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_data())
        .map_err(|e| format!("MachineRecordingBoundary: header sync: {e}"))?;
    RECORDING.with(|recording| {
        *recording.borrow_mut() = Some(Drain {
            writer: file,
            failure: None,
        })
    });
    Ok(())
}
pub fn drain(vm: &slr_vm::host::VmHost) -> Result<(), String> {
    RECORDING.with(|recording| {
        let mut recording = recording.borrow_mut();
        let Some(recording) = recording.as_mut() else {
            return Ok(());
        };
        recording.drain(&vm.machine_trace(), |file| file.sync_data())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::machine::Event;
    #[test]
    fn lossless_batches_retain_descriptors_other_tags_and_serialized_source() {
        let trace = Trace::default();
        trace.start().unwrap();
        for identity in 0..4100 {
            trace.note(Event::ThreadCreate { identity });
        }
        trace.note(Event::FileRead {
            identity: 4100,
            bytes: vec![0, 255, 10, 13],
        });
        trace.note(Event::ArrayClear {
            word: 0x1000,
            before: vec![
                slr_vm::values::Value {
                    tag: slr_vm::values::TypeTag::intern("[Ljava.io.Controller;"),
                    payload: 0xffc00001,
                },
                slr_vm::values::Value {
                    tag: slr_vm::values::TypeTag::Other(255),
                    payload: 0xdeadbeef,
                },
            ],
        });
        let mut drain = Drain {
            writer: Vec::new(),
            failure: None,
        };
        let mut syncs = 0;
        drain
            .drain(&trace, |_| {
                syncs += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(syncs, 2);
        assert!(trace.pending(usize::MAX).is_empty());
        let rows = drain
            .writer
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 4102);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["sequence"], i as u64);
        }
        assert_eq!(
            rows[4100]["event"]["FileRead"]["bytes"],
            serde_json::json!([0, 255, 10, 13])
        );
        let values = &rows[4101]["event"]["ArrayClear"]["before"];
        assert_eq!(values[0]["tag"]["Array"], "[Ljava.io.Controller;");
        assert_eq!(values[0]["payload"], 0xffc00001u32);
        assert_eq!(values[1]["tag"]["Other"], 255);
        assert_eq!(values[1]["payload"], 0xdeadbeefu32);
    }
    struct Broken {
        bytes: usize,
    }
    impl Write for Broken {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.bytes > 0 {
                return Err(std::io::Error::other("injected write failure"));
            }
            let n = buf.len().min(8);
            self.bytes += n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn partial_write_and_sync_failures_never_acknowledge_or_retry_a_prefix() {
        let trace = Trace::default();
        trace.start().unwrap();
        trace.note(Event::ThreadCreate { identity: 17 });
        let original = trace.pending(usize::MAX);
        let mut broken = Drain {
            writer: Broken { bytes: 0 },
            failure: None,
        };
        let first = broken
            .drain(&trace, |_| panic!("sync after failed write"))
            .unwrap_err();
        let written = broken.writer.bytes;
        assert_eq!(broken.drain(&trace, |_| Ok(())).unwrap_err(), first);
        assert_eq!(broken.writer.bytes, written);
        assert_eq!(trace.pending(usize::MAX), original);
        let mut failed_sync = Drain {
            writer: Vec::new(),
            failure: None,
        };
        assert!(failed_sync
            .drain(&trace, |_| Err(std::io::Error::other(
                "injected sync failure"
            )))
            .is_err());
        let written = failed_sync.writer.len();
        assert!(failed_sync.drain(&trace, |_| Ok(())).is_err());
        assert_eq!(failed_sync.writer.len(), written);
        assert_eq!(trace.pending(usize::MAX), original);
    }
}
