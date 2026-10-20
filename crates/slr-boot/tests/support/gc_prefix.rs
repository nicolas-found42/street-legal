//! Original first sweep and first drain, compared by allocation identity/class.
use std::path::Path;

pub fn assert_original_first_sweep(repository: &Path, output: &Path) {
    let original: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            repository
                .join("re/out/headless/finalizer-investigation/long-observed/original-facts.json"),
        )
        .expect("generate the passive original first-sweep capture first"),
    )
    .unwrap();
    let history = &original["finalizer_history"];
    assert_eq!(
        history["truncated"], false,
        "original GC history is truncated"
    );
    assert_eq!(
        history["alters_execution"], false,
        "original GC capture alters execution"
    );
    assert_eq!(
        history["allocation_records"], history["allocation_total"],
        "original allocation history is truncated"
    );
    assert_eq!(
        history["events"].as_array().unwrap().len() as u64,
        history["total"].as_u64().unwrap(),
        "original event history is incomplete"
    );
    // The long capture predates array descriptor observation. Join its raw
    // allocation identity to the independently retained complete preframe capture.
    let known: serde_json::Value =
        serde_json::from_str(
            &std::fs::read_to_string(repository.join(
                "re/out/headless/finalizer-investigation/callers-observed/original-facts.json",
            ))
            .expect("generate the complete original preframe allocation capture first"),
        )
        .unwrap();
    let known = &known["finalizer_history"];
    assert_eq!(known["truncated"], false);
    assert_eq!(known["strings_pending"], 0);
    let allocations = known["allocations"].as_array().unwrap();
    assert_eq!(
        allocations.len() as u64,
        known["allocation_total"].as_u64().unwrap()
    );
    let original_identity = |event: &serde_json::Value| {
        let allocation = &event["allocation"];
        let ordinal = allocation["ordinal"].as_u64().unwrap();
        let complete = &allocations[usize::try_from(ordinal - 1).unwrap()];
        for field in ["ordinal", "word", "caller", "icount", "native_count"] {
            assert_eq!(
                allocation[field], complete[field],
                "original capture allocation {ordinal} {field}"
            );
        }
        let class = complete["class"].as_str().unwrap();
        assert_ne!(
            class, "?",
            "original allocation {ordinal} lacks class evidence"
        );
        if event["class"] != "?" {
            assert_eq!(
                event["class"], class,
                "original allocation {ordinal} class evidence"
            );
        }
        (ordinal, class.to_owned())
    };
    let events = history["events"].as_array().unwrap();
    let first = events
        .iter()
        .position(|event| event["operation"] == "enqueue")
        .expect("original capture contains no sweep");
    let queued: Vec<_> = events[first..]
        .iter()
        .take_while(|event| event["operation"] == "enqueue")
        .map(|event| {
            assert_eq!(
                event["flags"].as_u64().unwrap() & 9,
                0,
                "original object already marked/queued before sweep"
            );
            original_identity(event)
        })
        .collect();
    assert!(!queued.is_empty());
    let port: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(output.join("gc-runtime.json"))
            .expect("run the stock boot with GC tracing first"),
    )
    .unwrap();
    let port = port.as_array().unwrap();
    let sweep = port
        .iter()
        .position(|event| event["operation"] == "sweep")
        .expect("port contains no completed first sweep");
    let project = |object: &serde_json::Value| {
        (
            object["allocation_ordinal"].as_u64().unwrap(),
            object["class"].as_str().unwrap().to_owned(),
        )
    };
    let actual: Vec<_> = port[sweep]["queued"]
        .as_array()
        .unwrap()
        .iter()
        .map(project)
        .collect();
    assert_eq!(
        actual, queued,
        "first sweep allocation identity/class/order"
    );

    // Compare the original first drain as a cohort, without selecting its
    // quota or finalizer identities from the implementation under test.
    let first_finalize = events[first + queued.len()..]
        .iter()
        .position(|event| event["operation"] == "finalize")
        .map(|index| first + queued.len() + index)
        .expect("original capture contains no finalizer drain");
    let iteration = events[first_finalize]["loop_iterations"].as_u64().unwrap();
    let cohort: Vec<_> = events[first_finalize..]
        .iter()
        .take_while(|event| event["loop_iterations"].as_u64() == Some(iteration))
        .filter(|event| event["operation"] == "finalize")
        .map(original_identity)
        .collect();
    assert!(!cohort.is_empty());
    let actual: Vec<_> = port[sweep + 1..]
        .iter()
        .filter(|event| event["operation"] == "finalize")
        .take(cohort.len())
        .map(|event| project(&event["object"]))
        .collect();
    assert_eq!(actual, cohort, "first original finalizer drain order");
}
