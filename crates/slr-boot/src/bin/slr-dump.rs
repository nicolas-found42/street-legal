//! Read-only class inspector: pool entries and method bodies as node streams.
//!
//! `cargo run -q -p slr-boot --bin slr-dump -- <file.class> [method-substring]`
//! The pool text next to a node is the entry its immediate indexes, when it
//! indexes one; it is a reading aid, not a decode of the opword's meaning.
use slr_fmt_tufa::PoolEntry;
use slr_vm::vmclass::Loader;

fn describe(pool: &[PoolEntry], i: u32) -> String {
    let text = |ix: u32| match pool.get(ix as usize) {
        Some(PoolEntry::Str(s)) => s.clone(),
        other => format!("{other:?}"),
    };
    match pool.get(i as usize) {
        Some(PoolEntry::Str(s)) => format!("str {s:?}"),
        Some(PoolEntry::Class(c)) => format!("class {}", text(*c)),
        Some(PoolEntry::Type5 { a, b }) => {
            format!("call-ref [{}] [{}]", describe(pool, *a), describe(pool, *b))
        }
        Some(PoolEntry::Type7 { a, b }) => format!("name/type {} {}", text(*a), text(*b)),
        Some(PoolEntry::Member { name, extra }) => format!("member {} {extra}", text(*name)),
        Some(PoolEntry::Int(v)) => format!("int {v}"),
        Some(PoolEntry::Float(v)) => format!("float {v}"),
        Some(other) => format!("{other:?}"),
        None => String::new(),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("class file");
    let filter = args.next();
    let bytes = std::fs::read(&path).expect("read");
    let mut loader = Loader::new();
    let ids = loader.load_file(&bytes).expect("load");
    for (id, name) in ids {
        println!("CLASS {name}");
        let pool = loader.pool_of(id).unwrap().to_vec();
        for m in &loader.find_by_id(id).unwrap().methods {
            if filter
                .as_ref()
                .is_some_and(|f| !m.name.contains(f.as_str()))
            {
                continue;
            }
            println!(
                "METHOD {}{} flags={:x} native={}",
                m.name, m.sig, m.flags, m.is_native
            );
            if m.is_native {
                continue;
            }
            let Some(body) = loader
                .body_id_of(id, m.body as usize)
                .and_then(|b| loader.body(b))
            else {
                continue;
            };
            for (i, n) in body.iter().enumerate() {
                let hint = if slr_fmt_tufa::Node::has_imm(n.opword) {
                    describe(&pool, n.imm)
                } else {
                    String::new()
                };
                println!(
                    "  {i:3} {:04x} line={:<3} imm={:<6} {hint}",
                    n.opword, n.operand, n.imm
                );
            }
        }
    }
}
