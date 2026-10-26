use super::*;

fn support_thread(token: u64, name: &[u8]) -> Thread {
    let input = ThreadInit {
        jvm: 0x1000,
        priority: 10,
        flags: 1,
        name: 0x2000,
    };
    Thread {
        token,
        address: 0x3000,
        input,
        name: name.to_vec(),
        committed: true,
        frame: Some(Frame {
            token: token + 1,
            address: 0x4000,
            thread: token,
            input: FrameInit {
                thread: 0x3000,
                context: 0,
                pc: 0,
                receiver: 0,
                locals: 0,
            },
            released: false,
            saved_pc: 0,
        }),
        retiring: None,
        destroyed: false,
    }
}
fn pair(thread: Thread) -> [Receipt; 2] {
    let mut retired = thread.clone();
    retired.retiring = Some(0x81);
    retired.destroyed = true;
    [
        Receipt::Thread {
            thread,
            stage: 0,
            identity: None,
        },
        Receipt::Retire {
            thread: retired,
            stage: 0,
            identity: None,
        },
    ]
}
fn support() -> Vec<Receipt> {
    let mut source = vec![
        Receipt::Class(Observation::Dependencies {
            class: 0x5000,
            base: 0x6000,
            interfaces: vec![],
        }),
        Receipt::Class(Observation::StaticOwner {
            class: 0x5000,
            owner: 0x7000,
        }),
    ];
    source.extend(pair(support_thread(1, b"THRD-CLASSVAR-INI")));
    source.push(Receipt::Object(objects::Receipt::Object {
        token: 1,
        address: 0x8000,
        class: 0x5000,
    }));
    source.extend(pair(support_thread(3, b"THRD-INSTVAR-INI")));
    source
}
fn valid(source: &[Receipt]) -> bool {
    array_support_object(&source.iter().collect::<Vec<_>>(), 0x5000).is_ok()
}
#[test]
fn array_support_cannot_consume_unrelated_or_partial_source_lifetimes() {
    assert!(valid(&support()));
    for change in 0..7 {
        let mut source = support();
        let Receipt::Retire { thread, stage, .. } = &mut source[3] else {
            unreachable!()
        };
        match change {
            0 => thread.token ^= 1,
            1 => thread.frame.as_mut().unwrap().token ^= 1,
            2 => thread.destroyed = false,
            3 => thread.retiring = Some(1),
            4 => thread.name.push(b'x'),
            5 => *stage = 1,
            _ => thread.frame.as_mut().unwrap().input.context = 0x5000,
        }
        assert!(!valid(&source), "changed lifetime {change}");
    }
    let mut source = support();
    source.push(Receipt::Class(Observation::StaticOwner {
        class: 0x9000,
        owner: 0xa000,
    }));
    assert!(!valid(&source));
    let mut source = support();
    source.swap(2, 3);
    assert!(!valid(&source));
    assert!(!valid(&support()[..6]));
}

#[test]
fn array_support_rejects_a_changed_actual_caller_before_consuming_receipts() {
    for change in 0..3 {
        let mut m = Space::new();
        let jvm = m.alloc(0x100);
        let class = m.alloc(0x1d8);
        let module = m.alloc(100);
        let fields = m.alloc(16);
        let name = m.alloc(4);
        let descriptor = m.alloc(32);
        let object = m.alloc(28);
        let mut thread = support_thread(1, b"outer");
        thread.address = m.alloc(80);
        let mut frame = thread.frame.unwrap();
        frame.address = m.alloc(64);
        frame.input.thread = thread.address;
        thread.frame = Some(frame);
        m.w32(class, 0x5e7340);
        m.w32(class + 4, module);
        m.w32(class + 8, name);
        m.w32(class + 12, jvm);
        let static_owner = m.alloc(20);
        m.w32(class + 0x1d0, static_owner);
        m.w32(module + 0x54, fields);
        m.w32(name, descriptor);
        for (index, byte) in b"[I\0".iter().enumerate() {
            m.w8(descriptor + index as u32, *byte);
        }
        m.w32(thread.address + 0x20, frame.address);
        m.w32(thread.address + 0x24, 0x8000);
        m.w32(object + 12, class);
        let mut owners = Owners::default();
        owners
            .physical_threads
            .insert(thread.address, thread.clone());
        owners.physical_frames.insert(frame.address, frame);
        owners
            .observe(
                &m,
                jvm,
                Observation::ArraySupportBegin {
                    thread: thread.address,
                    class,
                },
            )
            .unwrap();
        match change {
            0 => m.w32(thread.address + 0x20, 0),
            1 => m.w32(thread.address + 0x24, 0x8008),
            _ => m.w8(descriptor + 1, b'F'),
        }
        let error = owners
            .observe(
                &m,
                jvm,
                Observation::ArraySupportEnd {
                    thread: thread.address,
                    class,
                    object,
                },
            )
            .unwrap_err();
        assert!(error.contains("completion caller/class differs"), "{error}");
        assert!(owners.array_scope.is_some());
        assert!(owners.issued.is_empty());
    }
}

fn native_scope(
    thread: u64,
    generation: u64,
    implementation: u32,
    result_descriptor: &[u8],
) -> BoundNative {
    let source = support_thread(generation, b"callback");
    BoundNative {
        thread,
        source: Native {
            thread: source.token,
            frame: source.frame.unwrap(),
            pc: 0,
            class: 0,
            method: 0,
            descriptor: result_descriptor.to_vec(),
            receiver: 0,
            implementation,
            info: 0,
            cells: vec![],
            reference_result: false,
        },
    }
}
#[test]
fn nested_native_return_rejects_wrong_owner_and_payload_without_consuming_scope() {
    let mut m = Space::new();
    let jvm = m.alloc(0x80);
    m.w32(jvm + 8, jvm);
    m.w32(jvm + 0x68, jvm);
    let mut metadata = Metadata::new(&m, jvm).unwrap();
    let mut owners = Owners::default();
    owners.natives.push(native_scope(10, 1, 0x47cc10, b"()I"));
    owners.natives.push(native_scope(20, 3, 0x47bfe0, b"()F"));
    owners.issued.push_back(Receipt::NativeReturn {
        thread: 3,
        implementation: 0x47bfe0,
        result: Some(0x42f60000),
    });
    for (thread, implementation, result, executed, error) in [
        (10, 0x47cc10, Some(0x42f60000), true, None),
        (20, 0x47bfe0, Some(0x42f60001), true, None),
        (20, 0x47bfe0, Some(0x42f60000), false, None),
        (20, 0x47bfe0, Some(0x42f60000), true, Some("failed".into())),
    ] {
        let event = Entry {
            sequence: 0,
            event: Event::NativeReturn {
                thread,
                implementation,
                executed,
                result,
                error,
            },
        };
        assert!(owners.apply(&m, &event, &mut metadata).is_err());
        assert_eq!(metadata.sequence, 0);
        assert_eq!(owners.natives.len(), 2);
        assert_eq!(owners.issued.len(), 1);
    }
    owners
        .apply(
            &m,
            &Entry {
                sequence: 0,
                event: Event::NativeReturn {
                    thread: 20,
                    implementation: 0x47bfe0,
                    executed: true,
                    result: Some(0x42f60000),
                    error: None,
                },
            },
            &mut metadata,
        )
        .unwrap();
    assert_eq!(metadata.sequence, 1);
    assert_eq!(owners.natives.len(), 1);
    assert!(owners.issued.is_empty());
    owners.issued.push_back(Receipt::NativeReturn {
        thread: 1,
        implementation: 0x47cc10,
        result: Some(7),
    });
    owners
        .apply(
            &m,
            &Entry {
                sequence: 1,
                event: Event::NativeReturn {
                    thread: 10,
                    implementation: 0x47cc10,
                    executed: true,
                    result: Some(7),
                    error: None,
                },
            },
            &mut metadata,
        )
        .unwrap();
    assert_eq!(metadata.sequence, 2);
    assert!(owners.natives.is_empty());
    assert!(owners.issued.is_empty());
}
