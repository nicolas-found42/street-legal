//! Original resource-id lookup 536820/537000 over the manager's raw MRU
//! cache: 0x1c-byte nodes (+4 next, +8 previous, +0c embedded weak reference
//! whose id is +14 and resource +18) between sentinels at manager+106e94,
//! a free list at +106eb0, and a tree search from the root list at +114.
use crate::native_install::{empty, first, unlink};
use slr_physics::space::Space;

const MRU: u32 = 0x106e94;
const FREE: u32 = 0x106eb0;
const ROOT: u32 = 0x114;

fn detach(m: &mut Space, node: u32) {
    let previous = m.r32(node + 8);
    let following = m.r32(node + 4);
    if previous != 0 && following != 0 {
        m.w32(following + 8, previous);
        m.w32(previous + 4, following);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}
/// Insert after a header's leading sentinel (5386e0 / inline at 53710f).
fn push_front(m: &mut Space, header: u32, node: u32) {
    let front = m.r32(header + 8);
    m.w32(front + 8, node);
    m.w32(node + 4, front);
    m.w32(node + 8, header + 4);
    m.w32(header + 8, node);
}

/// Original544ac0: depth-first search of a resource subtree by id.
pub fn find(m: &Space, resource: u32, id: u32) -> u32 {
    if id == m.r32(resource + 0x50) {
        return resource;
    }
    let mut child = m.r32(resource + 0x20);
    if m.r32(child + 4) == 0 {
        return 0;
    }
    while child != 0 {
        let found = find(m, child, id);
        if found != 0 {
            return found;
        }
        child = m.r32(child + 4);
        if child == 0 || m.r32(child + 4) == 0 {
            return 0;
        }
    }
    0
}

/// Original544b10: like544ac0 below the root, but only descending into
/// children of the requested kind.
pub fn find_kind(m: &Space, resource: u32, id: u32, kind: u32) -> u32 {
    if id == m.r32(resource + 0x50) {
        return resource;
    }
    let mut child = m.r32(resource + 0x20);
    if m.r32(child + 4) == 0 {
        return 0;
    }
    while child != 0 {
        if m.r32(child + 0x4c) == kind {
            if id == m.r32(child + 0x50) {
                return child;
            }
            let mut grandchild = m.r32(child + 0x20);
            if m.r32(grandchild + 4) != 0 {
                while grandchild != 0 {
                    let found = find(m, grandchild, id);
                    if found != 0 {
                        return found;
                    }
                    grandchild = m.r32(grandchild + 4);
                    if grandchild == 0 || m.r32(grandchild + 4) == 0 {
                        break;
                    }
                }
            }
        }
        child = m.r32(child + 4);
        if child == 0 || m.r32(child + 4) == 0 {
            return 0;
        }
    }
    0
}

/// Original536820 on the manager at global618d48.
pub fn lookup(m: &mut Space, id: u32, kind: u32, flags: u32) -> Result<u32, String> {
    let manager = m.r32(0x618d48);
    if id == 0 {
        return Ok(0);
    }
    let package = id >> 16;
    if package != 0xffff && package < m.r32(manager + 0xffe20) {
        let record = manager + 0xffe94 + package * 0x70;
        if m.r32(record + 0x50) == 0 {
            return Err("NativeResourceBoundary: original544170 package header load".into());
        }
    }
    Ok(cached(m, manager, id, kind, flags))
}

/// Original537000.
pub fn cached(m: &mut Space, manager: u32, id: u32, kind: u32, flags: u32) -> u32 {
    let head = manager + MRU;
    let mut node = first(m, head);
    let mut hit = 0;
    while node != 0 {
        let following = {
            let next = m.r32(node + 4);
            if next == 0 || m.r32(next + 4) == 0 {
                0
            } else {
                next
            }
        };
        if m.r32(node + 0x14) == 0 {
            // A released cache entry is recycled. Its 545fc0(0) rebind is a
            // no-op: the embedded reference's id is already zero.
            let previous = m.r32(node + 8);
            if previous != 0 && m.r32(node + 4) != 0 {
                let next = m.r32(node + 4);
                m.w32(next + 8, previous);
                m.w32(previous + 4, next);
            }
            m.w32(node + 4, 0);
            m.w32(node + 8, 0);
            let tail = m.r32(manager + 0x106ec8);
            m.w32(tail + 4, node);
            m.w32(node + 4, manager + 0x106ec0);
            m.w32(node + 8, tail);
            m.w32(manager + 0x106ec8, node);
        } else if m.r32(node + 0x14) == id {
            hit = node;
            break;
        }
        node = following;
    }
    if hit != 0 {
        if kind != 0 {
            let resource = m.r32(hit + 0x18);
            let actual = if resource == 0 {
                0
            } else {
                m.r32(resource + 0x4c)
            };
            if kind != actual {
                return 0;
            }
        }
        detach(m, hit);
        push_front(m, head, hit);
        let resource = m.r32(hit + 0x18);
        if resource != 0 {
            return resource;
        }
    }
    let mut found = 0;
    let root = manager + ROOT;
    if !empty(m, root) && m.r32(root + 8) != 0 {
        let start = m.r32(root + 8);
        found = if kind == 0 {
            find(m, start, id)
        } else {
            find_kind(m, start, id, kind)
        };
        if found != 0 {
            let free = manager + FREE;
            if !empty(m, free) && m.r32(free + 8) != 0 {
                let node = m.r32(free + 8);
                detach(m, node);
                let current = m.r32(node + 0x18);
                if current != found {
                    if current != 0 {
                        unlink(m, node + 0xc);
                    }
                    // 4290f0 on found+44, then the id store at 53719d.
                    let link = node + 0xc;
                    let head_link = m.r32(found + 0x48);
                    if head_link != 0 {
                        m.w32(head_link, link);
                    }
                    m.w32(link, 0);
                    m.w32(link + 4, head_link);
                    m.w32(found + 0x48, link);
                    m.w32(link + 12, found);
                    m.w32(link + 8, m.r32(found + 0x50));
                }
                push_front(m, head, node);
                return found;
            }
            // Reuse the least recently used entry (538700/4a5d00/429060).
            let node = if m.r32(m.r32(head + 8) + 4) != 0 {
                m.r32(head + 0x18)
            } else {
                0
            };
            detach(m, node);
            crate::native_install::bind(m, node + 0xc, found);
            push_front(m, head, node);
            return found;
        }
    }
    if flags & 1 == 0 {
        // 54f580 realloc of the missing-id array, then append.
        let count = m.r32(manager + 0x106ecc);
        let old = m.r32(manager + 0x106ed0);
        let array = m.malloc(count * 4 + 4);
        for i in 0..count {
            m.w32(array + i * 4, m.r32(old + i * 4));
        }
        m.w32(manager + 0x106ed0, array);
        m.w32(array + count * 4, id);
        m.w32(manager + 0x106ecc, count + 1);
    }
    found
}
