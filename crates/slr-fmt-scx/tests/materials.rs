use std::collections::BTreeMap;
#[test]
fn stock_v4_material_records_account_for_every_payload_byte() {
    let mut trailing: BTreeMap<usize, usize> = BTreeMap::new();
    let (mut files, mut mats) = (0, 0);
    let mut kinds: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut tags: BTreeMap<u32, usize> = BTreeMap::new();
    let root = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock")
        });
    if !root.is_dir() {
        assert!(
            std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none(),
            "stock fixtures required"
        );
        eprintln!("SLR_ORACLE_SKIPPED: stock material fixtures absent");
        return;
    }
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("scx") || e.eq_ignore_ascii_case("scy"))
            {
                continue;
            }
            let data = std::fs::read(&p).unwrap();
            if let slr_fmt_scx::Scx::V4 { materials, .. } =
                slr_fmt_scx::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
            {
                files += 1;
                for m in &materials {
                    mats += 1;
                    *trailing.entry(m.trailing.len()).or_default() += 1;
                    *kinds.entry((m.kind, m.flags)).or_default() += 1;
                    for q in &m.params {
                        *tags.entry(q.tag() >> 24).or_default() += 1;
                    }
                }
            }
        }
    }
    assert_eq!(files, 2531);
    assert_eq!(mats, 4165);
    assert_eq!(trailing, BTreeMap::from([(0, 4165)]));
    assert_eq!(
        tags,
        BTreeMap::from([(0, 5120), (1, 2486), (5, 10), (6, 5117), (8, 3528)])
    );
    assert_eq!(kinds.values().sum::<usize>(), mats);
}
