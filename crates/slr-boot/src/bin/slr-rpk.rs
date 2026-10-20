//! Read-only RPK inspector: `slr-rpk <file.rpk> [type-id-hex]` lists entries or
//! prints one entry's descriptor text.
use slr_fmt_rpak::Package;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("rpk path");
    let package = Package::open(&path).expect("parse rpk");
    match args.next() {
        None => {
            for link in &package.links {
                println!(
                    "LINK {:#010x} {}",
                    link.package_id_shifted,
                    link.name.trim_end_matches('\0')
                );
            }
            for e in &package.entries {
                println!(
                    "ENTRY id={:#06x} super={:#010x} type={} flags={:#04x} rsd={}+{} alias={:?}",
                    e.type_id,
                    e.super_id,
                    e.typeof_,
                    e.flags,
                    e.rsd_offset,
                    e.rsd_size,
                    e.alias.trim_end_matches('\0')
                );
            }
        }
        Some(id) => {
            let id = u32::from_str_radix(id.trim_start_matches("0x"), 16).expect("hex id");
            let e = package
                .entries
                .iter()
                .find(|e| e.type_id == id)
                .expect("entry");
            println!("{}", package.descriptor(e));
        }
    }
}
