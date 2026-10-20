//! Measured frontend header registration (544010/536900/536a10).
//!
//! This installs the complete pinned frontend package in header order, with
//! declared empty foreign system roots, as in the original Unicorn profile.
//! Descriptor text, materialization and the foreign roots' own hierarchy are
//! deliberately unavailable. Existing foreign roots and retained links survive.
use crate::node::{Container, ResHost};
use slr_fmt_rpak::Package;
use std::collections::HashMap;

/// Header payload retained without parsing or materializing its descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub descriptor_offset: u32,
    pub descriptor_size: u32,
    pub alias: Option<String>,
    /// Original +0x68 after inactive-list insertion.
    pub range_bits: u32,
}

/// Installed IDs are replay-local; foreign roots have no evidenced hierarchy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub ids: Vec<u32>,
    pub declared_foreign_roots: Vec<u32>,
}

const KINDS: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
];
fn flags(kind: u8, raw: u8) -> u32 {
    u32::from(raw) | if matches!(kind, 3 | 5) { 0x100 } else { 0 }
}

fn normalise(name: &str) -> String {
    name.trim_end_matches('\0')
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// Register a whole package's headers transactionally, in header order.
///
/// The registration algorithm (`FUN_00544010` header conversion,
/// `FUN_00536900` constructors, `FUN_00536a10` child insertion) was replayed
/// against the original for the 279 frontend headers; other packages use the
/// same code path but have not been replayed. Foreign parents (entries whose
/// `super_id` names a linked package) resolve through that package's link to a
/// package in `packages`; a foreign entry that is not registered yet becomes a
/// declared empty root, and registering its own package later adopts it.
/// Descriptor text, materialisation and the foreign roots' own hierarchy are
/// deliberately unavailable.
/// exe: FUN_00544010, FUN_00536900, FUN_00536a10, FUN_00537de0
pub fn install_package(
    host: &mut ResHost,
    packages: &[(String, Package)],
    index: usize,
) -> Result<Installed, String> {
    let package = &packages.get(index).ok_or("package index out of range")?.1;
    if index == 0 {
        return Err("the system package is registered by the boot driver".into());
    }
    let package_index = u16::try_from(index).map_err(|_| "package index too large")?;
    let prefix = u32::from(package_index) << 16;
    let mut staged = host.clone();
    let mut known = HashMap::new();
    let mut declared_foreign_roots = Vec::new();
    let mut resolve_foreign =
        |staged: &mut ResHost, known: &mut HashMap<u32, u32>, parent: u32| -> Result<u32, String> {
            let link = package
                .links
                .iter()
                .find(|l| l.package_id_shifted == parent & 0xffff_0000)
                .ok_or("foreign resource parent link missing")?;
            let name = normalise(&link.name);
            let Some(foreign_index) = packages.iter().position(|(n, _)| *n == name) else {
                // The linked package is not part of the install (stock links
                // `obstacles\\*.rpk`, which the media does not ship): its roots
                // become declared empty placeholders so child headers still
                // register. What the original does here is unobserved.
                let id = (0x8000u32 + u32::from(package_index)) << 16 | (parent & 0xffff);
                if let Some(word) = known.get(&id) {
                    return Ok(*word);
                }
                let word = if let Some(word) = staged.container_for_id(id) {
                    word
                } else {
                    let mut c = Container::new(8, id);
                    c.flags = 0;
                    let word = staged.add_container(c);
                    staged.bind_id(id, word);
                    declared_foreign_roots.push(id);
                    word
                };
                known.insert(id, word);
                return Ok(word);
            };
            let id = ((foreign_index as u32) << 16) | (parent & 0xffff);
            if let Some(word) = known.get(&id) {
                return Ok(*word);
            }
            let entry = packages[foreign_index]
                .1
                .entries
                .iter()
                .find(|e| e.type_id == parent & 0xffff)
                .ok_or("foreign resource root missing")?;
            if !KINDS.contains(&entry.typeof_) {
                return Err("foreign resource constructor is not measured".into());
            }
            let word = if let Some(word) = staged.container_for_id(id) {
                let c = staged.container(word).ok_or("stale foreign root")?;
                if c.type_id != i32::from(entry.typeof_) {
                    return Err("existing foreign root has a different kind".into());
                }
                word
            } else {
                let mut c = Container::new(i32::from(entry.typeof_), id);
                c.flags = flags(entry.typeof_, entry.flags);
                let word = staged.add_container(c);
                staged.bind_id(id, word);
                declared_foreign_roots.push(id);
                word
            };
            known.insert(id, word);
            Ok(word)
        };
    let mut ids = Vec::new();
    for entry in &package.entries {
        let id = prefix | entry.type_id;
        if !KINDS.contains(&entry.typeof_) || entry.type_id > 0xffff {
            return Err(format!(
                "unmeasured resource kind {} for {id:#010x}",
                entry.typeof_
            ));
        }
        let parent = if entry.super_id >> 16 != 0 {
            resolve_foreign(&mut staged, &mut known, entry.super_id)?
        } else if entry.super_id == 0 {
            return Err(format!("root entry {id:#010x} has no parent container"));
        } else {
            *known
                .get(&(prefix | entry.super_id))
                .ok_or_else(|| format!("parent of {id:#010x} is not installed in header order"))?
        };
        let header = Header {
            descriptor_offset: entry.rsd_offset,
            descriptor_size: entry.rsd_size,
            alias: (entry.typeof_ == 8 && !entry.alias.is_empty()).then(|| entry.alias.clone()),
            range_bits: 0,
        };
        let word = if let Some(word) = staged.container_for_id(id) {
            // A declared root registered by an earlier package.
            let c = staged.container_mut(word).ok_or("stale adopted root")?;
            if c.type_id != i32::from(entry.typeof_) || c.header.is_some() {
                return Err(format!("conflicting resource {id:#010x}"));
            }
            c.load_state = 0x40;
            c.header = Some(header);
            word
        } else {
            let mut c = Container::new(i32::from(entry.typeof_), id);
            c.flags = flags(entry.typeof_, entry.flags);
            c.load_state = 0x40;
            c.header = Some(header);
            let word = staged.add_container(c);
            staged.append_child(parent, word)?;
            staged.bind_id(id, word);
            word
        };
        known.insert(id, word);
        ids.push(id);
    }
    *host = staged;
    Ok(Installed {
        ids,
        declared_foreign_roots,
    })
}
