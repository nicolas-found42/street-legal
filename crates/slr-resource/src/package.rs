//! Measured frontend header registration (544010/536900/536a10).
//!
//! This installs the complete pinned frontend package in header order, with
//! declared empty foreign system roots, as in the original Unicorn profile.
//! Descriptor text, materialization and the foreign roots' own hierarchy are
//! deliberately unavailable. Existing foreign roots and retained links survive.
use crate::node::{Container, ResHost};
use sha2::{Digest, Sha256};
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

const KINDS: &[u8] = &[3, 5, 6, 7, 8, 9, 13, 14];
fn flags(kind: u8, raw: u8) -> u32 {
    u32::from(raw) | if matches!(kind, 3 | 5) { 0x100 } else { 0 }
}

/// Register the pinned measured frontend profile transactionally. This is not
/// a general package loader or an assertion that foreign roots are truly empty.
/// exe: FUN_00544010, FUN_00536900, FUN_00536a10, FUN_00537de0
pub fn install_frontend(
    host: &mut ResHost,
    frontend: &Package,
    system: &Package,
    package_index: u16,
) -> Result<Installed, String> {
    for (package, expected) in [
        (
            frontend,
            "b2843513dc2440e8c23658066bf5b1b361016a52ea3fc435a2b8f56d34b55194",
        ),
        (
            system,
            "2af9adde7bee0ec0216f726facee5ed6005e2b30e6216928353a0f75f834a10c",
        ),
    ] {
        if format!("{:x}", Sha256::digest(package.data())) != expected {
            return Err("resource metadata profile requires the measured stock packages".into());
        }
    }
    if package_index == 0 || frontend.entries.len() != 279 {
        return Err("invalid frontend metadata profile identity".into());
    }
    let prefix = u32::from(package_index) << 16;
    let mut staged = host.clone();
    let mut known = HashMap::new();
    let mut foreign: Vec<_> = frontend
        .entries
        .iter()
        .filter(|e| e.super_id >> 16 != 0)
        .map(|e| e.super_id)
        .collect();
    foreign.sort_unstable();
    foreign.dedup();
    let mut declared_foreign_roots = Vec::new();
    for parent in foreign {
        let link = frontend
            .links
            .iter()
            .find(|l| l.package_id_shifted == parent & 0xffff0000)
            .ok_or("foreign resource parent link missing")?;
        if link.name.trim_end_matches('\0') != "system.rpk" {
            return Err("foreign resource parent is outside measured system roots".into());
        }
        let id = parent & 0xffff;
        let entry = system
            .entries
            .iter()
            .find(|e| e.type_id == id)
            .ok_or("foreign resource root missing")?;
        if !KINDS.contains(&entry.typeof_) {
            return Err("foreign resource constructor is not measured".into());
        }
        let word = if let Some(word) = staged.container_for_id(id) {
            let c = staged.container(word).ok_or("stale foreign root")?;
            if c.type_id != i32::from(entry.typeof_) || c.first_child != 0 {
                return Err(
                    "existing foreign root hierarchy is outside the measured empty-root profile"
                        .into(),
                );
            }
            word
        } else {
            let mut c = Container::new(i32::from(entry.typeof_), id);
            c.flags = flags(entry.typeof_, entry.flags);
            let word = staged.add_container(c);
            staged.bind_id(id, word);
            word
        };
        known.insert(id, word);
        declared_foreign_roots.push(id);
    }
    let mut ids = Vec::new();
    for entry in &frontend.entries {
        let id = prefix | entry.type_id;
        if !KINDS.contains(&entry.typeof_)
            || entry.type_id > 0xffff
            || staged.container_for_id(id).is_some()
        {
            return Err(format!(
                "unmeasured or conflicting frontend resource {id:#010x}"
            ));
        }
        let parent_id = if entry.super_id >> 16 != 0 {
            entry.super_id & 0xffff
        } else {
            prefix | entry.super_id
        };
        let parent = *known
            .get(&parent_id)
            .ok_or("frontend parent is not installed in header order")?;
        let mut c = Container::new(i32::from(entry.typeof_), id);
        c.flags = flags(entry.typeof_, entry.flags);
        c.load_state = 0x40;
        c.header = Some(Header {
            descriptor_offset: entry.rsd_offset,
            descriptor_size: entry.rsd_size,
            alias: (entry.typeof_ == 8 && !entry.alias.is_empty()).then(|| entry.alias.clone()),
            range_bits: 0,
        });
        let word = staged.add_container(c);
        staged.append_child(parent, word)?;
        staged.bind_id(id, word);
        known.insert(id, word);
        ids.push(id);
    }
    *host = staged;
    Ok(Installed {
        ids,
        declared_foreign_roots,
    })
}
