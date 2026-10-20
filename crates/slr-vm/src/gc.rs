//! Incremental object marking and sweep (`417f80`, `418970`, `408500`).
//!
//! The embedding owns root discovery, script finalizer execution and physical
//! destruction. Value-cell release does not change object reachability.
use std::collections::HashMap;

const MARKED: u32 = 1;
const FINALIZED: u32 = 2;
const DISABLED: u32 = 4;
const QUEUED: u32 = 8;

/// References in storage order. Arrays are marked eagerly, whereas ordinary
/// objects enter the original LIFO worklist and consume its 250-object budget.
#[derive(Clone, Debug)]
pub enum Edges {
    Object(Vec<u32>),
    Array(Vec<u32>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_chain_mark_steps_and_sweep_replay() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/headless/finalizer-investigation/gc-probe.json");
        let row: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("generate re/oracle/gc_probe.py output first"),
        )
        .unwrap();
        for (source, roots) in [(&row, vec![1]), (&row["wide"], (1..=502).collect())] {
            let mut gc = Collector::new();
            for word in 1..=504 {
                gc.allocated(word);
            }
            let mut edges = |word| Edges::Object(if word < 502 { vec![word + 1] } else { vec![] });
            for expected in source["rows"].as_array().unwrap() {
                let phase = gc.collect(roots.iter().copied(), &mut edges);
                let result = match phase {
                    Step::Started | Step::Marking => u32::MAX,
                    Step::Swept { ref queued } => queued.len() as u32,
                    Step::Idle => panic!("explicit collector must run"),
                };
                assert_eq!(result as u64, expected["result"].as_u64().unwrap());
                assert_eq!(gc.active(), expected["active"].as_bool().unwrap());
                assert_eq!(gc.pending() as u64, expected["pending"].as_u64().unwrap());
                let marked = (1..=504)
                    .filter(|word| gc.flags(*word).unwrap() & MARKED != 0)
                    .count();
                assert_eq!(marked as u64, expected["marked"].as_u64().unwrap());
                assert_eq!(
                    gc.allocation_counter() as u32 as u64,
                    expected["allocation_counter"].as_u64().unwrap()
                );
                assert_eq!(
                    gc.mark_bytes() as u64,
                    expected["mark_bytes"].as_u64().unwrap()
                );
                if let Step::Swept { queued } = phase {
                    assert_eq!(serde_json::to_value(queued).unwrap(), expected["queued"]);
                }
            }
        }
    }

    #[test]
    fn strict_threshold_and_allocations_during_mark() {
        let mut gc = Collector::new();
        let mut edges = |_| Edges::Object(vec![]);
        for word in 1..=1000 {
            gc.allocated(word);
        }
        assert_eq!(gc.step([], &mut edges), Step::Idle);
        gc.allocated(1001);
        assert_eq!(gc.step([], &mut edges), Step::Started);
        gc.allocated(1002);
        assert_eq!(gc.flags(1002), Some(MARKED));
        assert_eq!(
            gc.step([], &mut edges),
            Step::Swept {
                queued: (1..=1001).collect()
            }
        );
        assert_eq!(gc.flags(1002), Some(0));
        assert_eq!(gc.allocation_counter(), 2);
        assert_eq!(gc.step([], &mut edges), Step::Idle);
    }

    #[test]
    fn arrays_are_eager_and_enable_gc_prepends() {
        let mut gc = Collector::new();
        for word in 1..=4 {
            gc.allocated(word);
        }
        let mut edges = |word| match word {
            1 => Edges::Array(vec![2, 3]),
            2 => Edges::Object(vec![4]),
            _ => Edges::Object(vec![]),
        };
        assert_eq!(gc.collect([1], &mut edges), Step::Started);
        assert_eq!(gc.pending(), 2);
        gc.mark(1, &mut edges); // Array and object cycles cannot duplicate work.
        assert_eq!(gc.pending(), 2);
        assert_eq!(gc.collect([], &mut edges), Step::Swept { queued: vec![] });
        gc.disable(2);
        gc.enable(2);
        gc.collect([], &mut edges);
        assert_eq!(
            gc.collect([], &mut edges),
            Step::Swept {
                queued: vec![2, 1, 3, 4]
            }
        );
        assert!(gc.finalize(2));
        assert!(!gc.finalize(2));
        gc.revive(2);
        assert_eq!(gc.flags(2), Some(FINALIZED));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Started,
    Marking,
    Swept { queued: Vec<u32> },
}

/// Object-list order is significant: allocation appends, enableGC prepends,
/// and sweep queues unmarked objects in that list's forward order.
pub struct Collector {
    flags: HashMap<u32, u32>,
    live: Vec<u32>,
    work: Option<Vec<u32>>,
    capacity: usize,
    allocations: i32,
    threshold: i32,
    mark_bytes: u32,
}

impl Default for Collector {
    fn default() -> Self {
        Self {
            flags: HashMap::new(),
            live: Vec::new(),
            work: None,
            capacity: 0,
            allocations: 0,
            threshold: 1000,
            mark_bytes: 0,
        }
    }
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructor407da0 marks allocations born during an active collection.
    pub fn allocated(&mut self, word: u32) {
        assert!(word != 0 && !self.flags.contains_key(&word));
        self.flags.insert(word, u32::from(self.work.is_some()));
        self.live.push(word);
        self.allocations = self.allocations.wrapping_add(1);
    }

    pub fn active(&self) -> bool {
        self.work.is_some()
    }
    pub fn allocation_counter(&self) -> i32 {
        self.allocations
    }
    pub fn mark_bytes(&self) -> u32 {
        self.mark_bytes
    }
    pub fn pending(&self) -> usize {
        self.work.as_ref().map_or(0, Vec::len)
    }
    pub fn flags(&self, word: u32) -> Option<u32> {
        self.flags.get(&word).copied()
    }

    /// A reference write barrier (`41ab60`) calls this only while active.
    /// Supplied roots follow the embedding's original discovery order.
    pub fn mark(&mut self, word: u32, edges: &mut impl FnMut(u32) -> Edges) {
        if self.work.is_none() {
            return;
        }
        // Explicit stack preserves reverse array element traversal without a
        // Rust recursion limit for nested reference arrays.
        let mut incoming = vec![word];
        while let Some(word) = incoming.pop() {
            let Some(flags) = self.flags.get_mut(&word) else {
                continue;
            };
            if *flags & (MARKED | QUEUED) != 0 {
                continue;
            }
            *flags |= MARKED;
            match edges(word) {
                Edges::Object(_) => {
                    let work = self.work.as_mut().unwrap();
                    if work.len() + 1 > self.capacity {
                        self.capacity += work.len() + 1;
                    }
                    work.push(word);
                }
                Edges::Array(references) => incoming.extend(references),
            }
        }
    }

    /// One original automatic GC opportunity, after the finalizer drain.
    /// A start scans roots but performs no worklist step. Later opportunities
    /// process at most250 ordinary objects; the completing step sweeps at once.
    pub fn step(
        &mut self,
        roots: impl IntoIterator<Item = u32>,
        edges: &mut impl FnMut(u32) -> Edges,
    ) -> Step {
        if self.allocations <= self.threshold {
            return Step::Idle;
        }
        self.collect(roots, edges)
    }

    /// Explicit System.gc uses the same phase machine without the threshold.
    pub fn collect(
        &mut self,
        roots: impl IntoIterator<Item = u32>,
        edges: &mut impl FnMut(u32) -> Edges,
    ) -> Step {
        if self.work.is_none() {
            self.work = Some(Vec::new());
            self.capacity = 64;
            for root in roots {
                self.mark(root, edges);
            }
            return Step::Started;
        }
        for _ in 0..250 {
            let Some(word) = self.work.as_mut().unwrap().pop() else {
                break;
            };
            let references = match edges(word) {
                Edges::Object(references) | Edges::Array(references) => references,
            };
            for reference in references.into_iter().rev() {
                self.mark(reference, edges);
            }
        }
        if self.pending() != 0 {
            return Step::Marking;
        }
        self.mark_bytes = self.capacity.wrapping_mul(4) as u32;
        self.work = None;
        let mut queued = Vec::new();
        self.live.retain(|word| {
            let flags = self.flags.get_mut(word).unwrap();
            if *flags & MARKED != 0 {
                *flags &= !MARKED;
                true
            } else {
                *flags |= QUEUED;
                queued.push(*word);
                false
            }
        });
        self.allocations = self.allocations.wrapping_sub(self.threshold);
        Step::Swept { queued }
    }

    pub fn disable(&mut self, word: u32) {
        let Some(flags) = self.flags.get_mut(&word) else {
            return;
        };
        if *flags & DISABLED == 0 {
            *flags |= DISABLED;
            self.live.retain(|candidate| *candidate != word);
        }
    }

    pub fn enable(&mut self, word: u32) {
        let Some(flags) = self.flags.get_mut(&word) else {
            return;
        };
        if *flags & DISABLED != 0 {
            *flags &= !DISABLED;
            self.live.insert(0, word);
        }
    }

    /// Interned queued strings can rejoin the live list (`4174a0`).
    /// The embedding must also remove this word from its finalizer queue.
    pub fn revive(&mut self, word: u32) {
        let Some(flags) = self.flags.get_mut(&word) else {
            return;
        };
        if *flags & QUEUED != 0 {
            *flags &= !QUEUED;
            self.live.push(word);
        }
        if self.work.is_some() {
            *flags |= MARKED;
        }
    }

    /// Returns whether408560 would execute the script finalizer. The embedding
    /// then clears reference fields and moves the record to its spare list.
    pub fn finalize(&mut self, word: u32) -> bool {
        let Some(flags) = self.flags.get_mut(&word) else {
            return false;
        };
        if *flags & FINALIZED != 0 {
            return false;
        }
        *flags |= FINALIZED;
        true
    }
}
