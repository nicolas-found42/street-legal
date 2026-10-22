//! Instance message routing. Virtual resource operations and script execution
//! stay live so a callback can change the envelope seen by the next query.

pub trait Host {
    fn read(&mut self, address: u32) -> Result<u32, String>;
    fn minimum(&mut self, resource: u32) -> Result<(), String>;
    fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String>;
    fn data(&mut self, resource: u32) -> Result<u32, String>;
    fn access(&mut self, resource: u32) -> Result<u32, String>;
    fn script(&mut self, envelope: u32, message: u32) -> Result<u32, String>;
    fn native(
        &mut self,
        payload: u32,
        body: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String>;
}

/// exe: FUN_00426470
pub fn query(
    host: &mut dyn Host,
    reference: u32,
    message: u32,
    argument: u32,
) -> Result<u32, String> {
    if host.read(reference + 8)? == 0 {
        return Ok(0);
    }
    let resource = host.read(reference + 12)?;
    let kind = if resource == 0 {
        0
    } else {
        host.read(resource + 0x4c)?
    };
    if kind != 1 && kind != 8 {
        return Ok(message);
    }
    if host.read(resource + 0x4c)? != 1 {
        host.minimum(resource)?;
    }
    let flags = if kind == 1 { 0xa0000000 } else { 0x80000000 };
    if host.prepare(resource, flags)? & 0x80000000 != 0 {
        return Ok(0);
    }
    let envelope = host.data(resource)?;
    if envelope == 0 {
        return Ok(0);
    }
    if kind == 8 {
        let payload = host.read(envelope + 12)?;
        return host.native(payload, 0, message, argument);
    }
    if host.read(envelope + 0x4c)? == 0 {
        return if host.read(envelope + 0x50)? == 0 {
            Ok(0)
        } else {
            host.script(envelope, message)
        };
    }
    let secondary = host.read(envelope + 0x44)?;
    if secondary == 0 {
        return Ok(0);
    }
    let data = host.access(secondary)?;
    if data == 0 {
        return Ok(0);
    }
    let payload = host.read(data + 12)?;
    // Access can invoke loading callbacks: the executable rereads the body
    // after that call rather than retaining its earlier nonzero test value.
    let body = host.read(envelope + 0x4c)?;
    host.native(payload, body, message, argument)
}

/// Query phase of original427210; the host owns its callback queues.
/// Save all three answers before publishing any registration. Each query
/// rereads the reference, resource kind and envelope after the previous call.
pub fn capabilities(host: &mut dyn Host, reference: u32) -> Result<[u32; 3], String> {
    Ok([
        query(host, reference, 8, 0)?,
        query(host, reference, 9, 0)?,
        query(host, reference, 0x1c, 0)?,
    ])
}

struct Runtime<'a>(&'a mut slr_vm::host::VmHost);

impl Host for Runtime<'_> {
    fn read(&mut self, address: u32) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| m.r32(address))
    }
    fn minimum(&mut self, resource: u32) -> Result<(), String> {
        crate::native_body::with_world_memory(|m| crate::native_assets::minimum(m, resource))?
    }
    fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| {
            crate::native_assets::prepare(
                m,
                resource,
                crate::native_resource_load::Access {
                    flags,
                    quality: 1f32.to_bits(),
                    priority: 0,
                    time: 0,
                },
            )
        })?
    }
    fn data(&mut self, resource: u32) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| crate::native_assets::data(m, resource))?
    }
    fn access(&mut self, resource: u32) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| {
            crate::native_assets::access(
                m,
                resource,
                crate::native_resource_load::Access {
                    flags: 0x80000000,
                    quality: 1f32.to_bits(),
                    priority: 0,
                    time: 0,
                },
            )
        })?
    }
    fn script(&mut self, envelope: u32, message: u32) -> Result<u32, String> {
        // 425a90 -> 4255e0 checks the script and its own secondary resource
        // again. Do not retain a memory lease while executing VM callbacks.
        let script = self.read(envelope + 0x50)?;
        if script == 0 {
            return Ok(0);
        }
        let secondary = self.read(envelope + 0x44)?;
        if secondary == 0 || self.access(secondary)? == 0 {
            return Ok(0);
        }
        let container = crate::native_body::logical_resource(envelope - 0xd8)
            .ok_or("getInfo envelope has no live resource identity")?;
        let script = self.read(envelope + 0x50)?;
        crate::engine::query_script(self.0, container, script, message)
    }
    fn native(
        &mut self,
        payload: u32,
        body: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| {
            let class = match m.r32(payload) {
                0x5f0f18 => "car",
                0x5f0f60 => "part",
                0x5f11e4 => "cursor",
                0x5f12f0 => "player",
                0x5f133c => "trigger",
                0x5f0e10 => "button",
                vtable => return Err(format!("unimplemented getInfo vtable {vtable:#x}")),
            };
            crate::native_message::template_query(m, payload, class, message, body, argument)
        })?
    }
}

/// Project the factory's live instance into the same raw envelope used by
/// native constructors. Native bodies are retained as written by those
/// constructors; script-only instances keep the constructor's zero body.
pub fn created_capabilities(
    vm: &mut slr_vm::host::VmHost,
    container: u32,
) -> Result<[u32; 3], String> {
    let template = crate::factory::instance_template_container(container)
        .ok_or("created instance has no retained template")?;
    let script = slr_resource::node::with_res(|h| {
        h.container(container)
            .and_then(|c| c.instance.as_ref())
            .map(|i| i.script)
    })
    .ok_or("created instance has no envelope")?;
    let reference = crate::native_body::with_world_memory(|m| {
        let raw = crate::native_body::raw_resource(m, container)?;
        let secondary = crate::native_body::raw_resource(m, template)?;
        if let Some(payload) = crate::native_body::template_payload(template) {
            m.w32(secondary + 0x8c, payload);
        }
        crate::native_part::bind_reference(m, raw + 0x110, secondary);
        m.w32(raw + 0x128, script);
        // Factory materialisation has already loaded the TUFA class or the
        // configured native template before it can allocate this instance.
        crate::native_resource_load::mark_loaded(m, secondary, 1)?;
        m.w32(secondary + 0x68, 1f32.to_bits());
        let reference = m.alloc(16);
        crate::native_part::bind_reference(m, reference, raw);
        Ok::<_, String>(reference)
    })??;
    let result = capabilities(&mut Runtime(vm), reference);
    crate::native_body::with_world_memory(|m| {
        crate::native_part::bind_reference(m, reference, 0);
    })?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use slr_physics::space::Space;

    struct Controlled<'a> {
        m: Space,
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Controlled<'_> {
        fn word(&self, name: &str) -> u32 {
            self.row[name].as_u64().unwrap() as u32
        }
        fn flag(&self, name: &str) -> bool {
            self.row["spec"][name].as_bool().unwrap()
        }
        fn mutation(&self) -> u64 {
            self.row["spec"]["mutation"].as_u64().unwrap()
        }
        fn answer(&self, message: u32) -> u32 {
            if self.flag("registration") {
                let index = [8, 9, 28].iter().position(|&v| v == message).unwrap();
                (self.row["spec"]["answers"].as_u64().unwrap() as u32 >> index) & 1
            } else {
                message ^ 0x12345678
            }
        }
        fn record(&mut self, name: &str, this: u32, args: &[u32]) {
            self.events
                .push(json!({"name":name,"this":this,"args":args}));
        }
    }
    impl Host for Controlled<'_> {
        fn read(&mut self, address: u32) -> Result<u32, String> {
            Ok(self.m.r32(address))
        }
        fn minimum(&mut self, resource: u32) -> Result<(), String> {
            self.record("minimum", resource, &[1f32.to_bits()]);
            Ok(())
        }
        fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String> {
            self.record("prepare", resource, &[flags, 0, 0]);
            Ok(self.row["spec"]["refused"].as_u64().unwrap() as u32)
        }
        fn data(&mut self, resource: u32) -> Result<u32, String> {
            self.record("data", resource, &[1f32.to_bits()]);
            Ok(if self.flag("data") {
                self.word("envelope")
            } else {
                0
            })
        }
        fn access(&mut self, resource: u32) -> Result<u32, String> {
            self.record("access", resource, &[0x80000000, 1f32.to_bits(), 0, 0]);
            match self.mutation() {
                4 => self.m.w32(self.word("envelope") + 0x4c, 0x200000f8),
                5 => self.m.w32(self.word("envelope") + 0x4c, 0),
                _ => (),
            }
            Ok(if self.flag("secondary_data") {
                self.word("secondary_data")
            } else {
                0
            })
        }
        fn script(&mut self, envelope: u32, message: u32) -> Result<u32, String> {
            self.events.push(json!({"name":"script",
                "args":[envelope,0x60cad4,1,2,message,0x4f]}));
            match self.mutation() {
                1 => self.m.w32(envelope + 0x4c, 0x200000fc),
                2 => self.m.w32(envelope + 0x50, 0),
                3 => self.m.w32(self.word("reference") + 8, 0),
                _ => (),
            }
            Ok(self.answer(message))
        }
        fn native(
            &mut self,
            payload: u32,
            body: u32,
            message: u32,
            argument: u32,
        ) -> Result<u32, String> {
            self.record("native", payload, &[body, message, argument]);
            Ok(self.answer(message))
        }
    }

    #[test]
    fn instance_routing_and_registration_match_original_mutating_graphs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/oracle/instance_query.jsonl");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run_vectors.py gen --suite instance-query");
            return;
        };
        let mut count = 0;
        for line in text.lines() {
            let vector: Value = serde_json::from_str(line).unwrap();
            let row = &vector["result"];
            let start = row["heap_start"].as_u64().unwrap() as u32;
            let mut host = Controlled {
                m: Space::new(),
                row,
                events: vec![],
            };
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                host.m
                    .w32(start + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let reference = host.word("reference");
            if host.flag("registration") {
                let answers = capabilities(&mut host, reference).unwrap();
                // Read the original's actual three intrusive lists, including
                // retained resource identity, rather than trusting the mask
                // used by the supplied callback. Raw list layout itself is
                // outside the host's Vec callback-queue projection.
                let context = host.word("context");
                let after = row["after"].as_array().unwrap();
                let read_original =
                    |address: u32| after[((address - start) / 4) as usize].as_u64().unwrap() as u32;
                for (index, offset) in [0, 0x1c, 0x38].into_iter().enumerate() {
                    let first = read_original(context + offset + 8);
                    let count = read_original(context + 0xc4 + index as u32 * 4);
                    assert_eq!(
                        count,
                        u32::from(answers[index] != 0),
                        "vector{} kind{index}",
                        vector["id"]
                    );
                    if count != 0 {
                        assert_eq!(read_original(first + 0x14), 123);
                        assert_eq!(read_original(first + 0x18), host.word("resource"));
                        assert_eq!(read_original(first + 4), context + offset + 0x10);
                    } else {
                        assert_eq!(first, context + offset + 0x10);
                    }
                }
            } else {
                assert_eq!(
                    query(&mut host, reference, 8, 37).unwrap(),
                    row["result"].as_u64().unwrap() as u32,
                    "vector{}",
                    vector["id"]
                );
                for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        host.m.r32(start + i as u32 * 4),
                        word.as_u64().unwrap() as u32,
                        "vector{} word{i}",
                        vector["id"]
                    );
                }
            }
            assert_eq!(json!(host.events), row["events"], "vector{}", vector["id"]);
            count += 1;
        }
        assert!(
            count >= 340,
            "complete routing/registration input grid required"
        );
    }
}
