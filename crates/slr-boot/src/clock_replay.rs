//! Strict replay of external counter inputs from a passive original capture.
//!
//! The tape contains QPC/QPF output words, never native return fixtures. The
//! existing `CounterClock` performs the original arithmetic. Request mismatches
//! poison the replay rather than skipping samples or returning a default clock.
use serde_json::Value;
use sha2::{Digest, Sha256};
use slr_lang::clock::{CounterClock, CounterState, PerformanceCounters};
use slr_vm::clock::ClockSite;
use slr_x87::{Precision, Rounding, X87};
use std::{collections::HashMap, fmt, fs, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayError(pub String);
impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for ReplayError {}
type Result<T> = std::result::Result<T, ReplayError>;
fn err(message: impl Into<String>) -> ReplayError {
    ReplayError(message.into())
}
fn number(v: &Value, key: &str) -> Result<u64> {
    v[key]
        .as_u64()
        .ok_or_else(|| err(format!("missing/invalid unsigned {key}")))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| err(format!("missing/invalid string {key}")))
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let size = fs::metadata(path)
        .map_err(|e| err(format!("{}: {e}", path.display())))?
        .len();
    if size > 64 * 1024 * 1024 {
        return Err(err("capture file exceeds 64MiB"));
    }
    fs::read(path).map_err(|e| err(format!("{}: {e}", path.display())))
}
fn json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|e| err(e.to_string()))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check_digest(bytes: &[u8], expected: &Value) -> Result<()> {
    if expected.as_str() != Some(digest(bytes).as_str()) {
        return Err(err("capture SHA256 mismatch"));
    }
    Ok(())
}

/// Remove only the original object's address suffix from generated thread names.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    if let Some((prefix, suffix)) = name.rsplit_once('<') {
        if let Some(address) = suffix.strip_suffix('>') {
            if prefix.starts_with('L')
                && prefix.ends_with(';')
                && !address.is_empty()
                && address.len() <= 8
                && address.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return prefix.to_owned();
            }
        }
    }
    name.to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadContext {
    pub id: u64,
    pub name: Option<String>,
    pub class: Option<String>,
    pub method: Option<String>,
    pub node_index: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestContext {
    pub thread: Option<ThreadContext>,
    pub driver_cursor: Option<ThreadContext>,
}
#[derive(Clone, Debug)]
struct CapturedThread {
    address: u64,
    terminal_initializer: bool,
    context: ThreadContext,
}
fn captured_thread(v: &Value) -> Result<Option<CapturedThread>> {
    if v.is_null() {
        return Ok(None);
    }
    fn optional_string(v: &Value, key: &str) -> Result<Option<String>> {
        if v[key].is_null() {
            Ok(None)
        } else {
            Ok(Some(text(v, key)?.to_owned()))
        }
    }
    Ok(Some(CapturedThread {
        address: number(v, "address")?,
        terminal_initializer: matches!(
            v["name"].as_str(),
            Some("THRD-CLASSVAR-INI" | "THRD-INSTVAR-INI")
        ) && number(v, "flags")? & 0x41 == 0x41,
        context: ThreadContext {
            id: 0,
            name: optional_string(v, "name")?.map(|n| normalize_name(&n)),
            class: optional_string(v, "class")?,
            method: optional_string(v, "method")?,
            node_index: if v["node_index"].is_null() {
                None
            } else {
                Some(number(v, "node_index")?)
            },
        },
    }))
}
#[derive(Clone, Debug)]
struct Sample {
    ordinal: u64,
    caller: u32,
    site: ClockSite,
    entry: u64,
    completed: u64,
    frequency: i64,
    counter: i64,
    baseline: i64,
    cw: u16,
    thread: Option<CapturedThread>,
    cursor: Option<CapturedThread>,
}
impl Sample {
    fn parse(v: &Value) -> Result<Self> {
        let caller = u32::try_from(number(v, "caller")?).map_err(|_| err("caller overflow"))?;
        let site = ClockSite::from_original_caller(caller)
            .ok_or_else(|| err(format!("unsupported clock caller {caller:#x}")))?;
        if text(v, "role")? != format!("{site:?}") {
            return Err(err("role/caller mismatch"));
        }
        let input = &v["external_input"];
        let signed = |key| -> Result<i64> {
            i64::try_from(number(input, key)?).map_err(|_| err("counter word exceeds signed i64"))
        };
        Ok(Self {
            ordinal: number(v, "request_ordinal")?,
            caller,
            site,
            entry: number(v, "entry_icount")?,
            completed: number(v, "completed_icount")?,
            frequency: signed("frequency")?,
            counter: signed("counter")?,
            baseline: signed("baseline")?,
            cw: u16::try_from(number(v, "expected_fpcw")?).map_err(|_| err("FPCW overflow"))?,
            thread: captured_thread(&v["thread_state"])?,
            cursor: captured_thread(&v["driver_cursor"])?,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Selection {
    pub next_request_ordinal: u64,
    pub baseline: i64,
    pub frequency: i64,
    pub accepted_remaining: usize,
}
/// Diagnostic description of the next expected request; values are never sampled
/// from this metadata by `read`'s caller.
#[derive(Clone, Debug)]
pub struct Pending {
    pub ordinal: u64,
    pub site: ClockSite,
    pub original_caller: u32,
    pub counter: i64,
    pub frequency: i64,
    pub baseline: i64,
    pub source_entry_icount: u64,
    pub source_completed_icount: u64,
    pub thread: Option<ThreadContext>,
    pub driver_cursor: Option<ThreadContext>,
}
pub struct Replay {
    samples: Vec<Sample>,
    position: usize,
    after_game_init: usize,
    segment_start: usize,
    accepted: usize,
    selected: bool,
    clock: Option<CounterClock>,
    bindings: HashMap<u64, u64>,
    reverse: HashMap<u64, u64>,
    failure: Option<ReplayError>,
    pub source_facts_sha256: String,
}
impl Replay {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let base = path.parent().ok_or_else(|| err("capture has no parent"))?;
        let manifest = json(&read_file(&base.join("clock-verification.json"))?)?;
        if manifest["truncated"] != false
            || number(&manifest, "pending_clocks")? != 0
            || manifest["context_errors"]
                .as_array()
                .is_none_or(|v| !v.is_empty())
        {
            return Err(err("incomplete clock capture"));
        }
        if number(&manifest, "events")? > number(&manifest, "event_limit")?
            || number(&manifest, "event_limit")? > 50000
        {
            return Err(err("invalid capture event bound"));
        }
        let bytes = read_file(path)?;
        check_digest(&bytes, &manifest["files"]["clock-inputs.jsonl"]["sha256"])?;
        if bytes.len() as u64 != number(&manifest["files"]["clock-inputs.jsonl"], "bytes")? {
            return Err(err("input byte count mismatch"));
        }
        let source = read_file(&base.join("observed/original-facts.json"))?;
        check_digest(&source, &manifest["source_facts_sha256"])?;
        let facts = json(&source)?;
        let history = &facts["clock_history"];
        if facts["alters_execution"] != false
            || history["alters_execution"] != false
            || facts["observer"] != "passive-unicorn-code-hooks"
            || number(history, "total")? != number(&manifest, "events")?
        {
            return Err(err("invalid passive source provenance"));
        }
        if history["truncated"] != false
            || number(history, "pending_clocks")? != 0
            || history["context_errors"]
                .as_array()
                .is_none_or(|v| !v.is_empty())
        {
            return Err(err("incomplete source history"));
        }
        let events = history["events"]
            .as_array()
            .ok_or_else(|| err("missing source events"))?;
        let originals: Vec<_> = events.iter().filter(|v| v["kind"] == "clock").collect();
        let rows = std::str::from_utf8(&bytes)
            .map_err(|e| err(e.to_string()))?
            .lines()
            .map(|l| json(l.as_bytes()))
            .collect::<Result<Vec<_>>>()?;
        if rows.len() != originals.len()
            || rows.len() as u64 != number(&manifest, "clock_requests")?
        {
            return Err(err("clock request count mismatch"));
        }
        let mut samples = Vec::with_capacity(rows.len());
        for (index, (row, original)) in rows.iter().zip(originals).enumerate() {
            for (export, source_key) in [
                ("caller", "caller"),
                ("entry_icount", "entry_icount"),
                ("completed_icount", "icount"),
                ("event_ordinal", "ordinal"),
                ("external_input", "external_input"),
                ("contexts", "contexts"),
                ("thread_state", "thread_state"),
                ("driver_cursor", "driver_cursor"),
                ("expected_fpcw", "fpcw"),
            ] {
                if row[export] != original[source_key] {
                    return Err(err(format!(
                        "request {} disagrees with source {export}",
                        index + 1
                    )));
                }
            }
            let sample = Sample::parse(row)?;
            if sample.ordinal != index as u64 + 1
                || sample.entry > sample.completed
                || samples
                    .last()
                    .is_some_and(|p: &Sample| p.completed > sample.entry)
            {
                return Err(err("unordered clock requests"));
            }
            samples.push(sample);
        }
        let phases = json(&read_file(&base.join("phase-index.json"))?)?;
        let phase = &phases["GameInit"];
        let creations = facts["resource_creations"]
            .as_array()
            .ok_or_else(|| err("missing resource creation boundary"))?;
        let init = creations
            .iter()
            .find(|c| c["name"] == "GameInit")
            .ok_or_else(|| err("missing original GameInit"))?;
        if phase["start_icount"] != init["icount"] || phase["end_icount"] != init["return_icount"] {
            return Err(err("GameInit phase/source mismatch"));
        }
        let boundary = number(init, "return_icount")?;
        let after_game_init = samples.partition_point(|s| s.entry < boundary);
        if after_game_init == 0
            || samples[after_game_init - 1].completed >= boundary
            || after_game_init as u64 != number(phase, "last_request_ordinal")?
            || samples
                .get(after_game_init)
                .is_none_or(|s| s.site != ClockSite::FramePhysicsEpoch)
        {
            return Err(err("invalid post-GameInit boundary"));
        }
        let first =
            samples.partition_point(|s| s.entry < number(phase, "start_icount").unwrap_or(0));
        if first as u64 + 1 != number(phase, "first_request_ordinal")?
            || (after_game_init - first) as u64 != number(phase, "clock_requests")?
        {
            return Err(err("GameInit request range mismatch"));
        }
        let accepted_icount = number(&manifest, "accepted_icount")?;
        let accepted = samples.partition_point(|s| s.entry < accepted_icount);
        if accepted as u64 != number(&manifest, "accepted_clock_requests")?
            || accepted <= after_game_init
            || samples[accepted - 1].completed >= accepted_icount
        {
            return Err(err("invalid accepted menu cutoff"));
        }
        let accepted_phase = &phases["ThroughAcceptedMenu"];
        if number(accepted_phase, "end_icount")? != accepted_icount
            || number(accepted_phase, "clock_requests")? != accepted as u64
            || number(accepted_phase, "last_request_ordinal")? != accepted as u64
        {
            return Err(err("accepted phase/source mismatch"));
        }
        let menu = facts["menu_history"]["snapshots"]
            .as_array()
            .ok_or_else(|| err("missing menu observations"))?;
        let menu = menu
            .iter()
            .find(|m| m["idle_frames"].as_u64().is_some_and(|n| n >= 300))
            .ok_or_else(|| err("missing accepted menu boundary"))?;
        if number(menu, "icount")? != accepted_icount
            || menu["complete"] != true
            || menu["thread_list_valid"] != true
            || menu["pending"].as_array().is_none_or(|v| !v.is_empty())
            || menu["unknown_frames"]
                .as_array()
                .is_none_or(|v| !v.is_empty())
        {
            return Err(err("invalid original menu acceptance"));
        }
        Ok(Self {
            samples,
            position: 0,
            after_game_init,
            segment_start: 0,
            accepted,
            selected: false,
            clock: None,
            bindings: HashMap::new(),
            reverse: HashMap::new(),
            failure: None,
            source_facts_sha256: text(&manifest, "source_facts_sha256")?.to_owned(),
        })
    }
    /// Select the earliest captured external clock request. No request is skipped.
    pub fn select_start(&mut self) -> Result<Selection> {
        self.select(0)
    }
    pub fn select_after_game_init(&mut self) -> Result<Selection> {
        self.select(self.after_game_init)
    }
    fn select(&mut self, position: usize) -> Result<Selection> {
        if self.selected || self.failure.is_some() {
            return Err(err("replay already selected or failed"));
        }
        self.position = position;
        self.segment_start = position;
        self.selected = true;
        let sample = &self.samples[self.position];
        self.clock = Some(CounterClock::from_state(CounterState {
            frequency: sample.frequency,
            start: sample.baseline,
            now: 0,
        }));
        Ok(Selection {
            next_request_ordinal: sample.ordinal,
            baseline: sample.baseline,
            frequency: sample.frequency,
            accepted_remaining: self.accepted - self.position,
        })
    }
    /// Validate that live execution reached the recorded phase boundary without
    /// moving the tape or discarding any mismatching requests.
    pub fn check_after_game_init(&self) -> Result<()> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !self.selected || self.position != self.after_game_init {
            return Err(err(format!(
                "GameInit clock boundary incomplete: position {} expected {}",
                self.position, self.after_game_init
            )));
        }
        Ok(())
    }
    #[must_use]
    pub fn pending(&self) -> Option<Pending> {
        self.samples.get(self.position).map(|s| Pending {
            ordinal: s.ordinal,
            site: s.site,
            original_caller: s.caller,
            counter: s.counter,
            frequency: s.frequency,
            baseline: s.baseline,
            source_entry_icount: s.entry,
            source_completed_icount: s.completed,
            thread: s.thread.as_ref().map(|t| t.context.clone()),
            driver_cursor: s.cursor.as_ref().map(|t| t.context.clone()),
        })
    }
    #[must_use]
    pub fn consumed(&self) -> usize {
        self.position.saturating_sub(self.segment_start)
    }
    pub fn finish_accepted(&self) -> Result<()> {
        if let Some(e) = &self.failure {
            return Err(e.clone());
        }
        if !self.selected || self.position != self.accepted {
            return Err(err(format!(
                "accepted clock boundary incomplete: position {} expected {}",
                self.position, self.accepted
            )));
        }
        Ok(())
    }
    pub fn read(
        &mut self,
        site: ClockSite,
        context: &RequestContext,
        precision: Precision,
        rounding: Rounding,
    ) -> Result<X87> {
        if let Some(e) = &self.failure {
            return Err(e.clone());
        }
        let result = self.try_read(site, context, precision, rounding);
        if let Err(e) = &result {
            self.failure = Some(e.clone());
        }
        result
    }
    /// Release an identity only when the embedding actually destroys that
    /// interpreter thread. Original allocation addresses can then be reused by
    /// a later lifetime without weakening request-context matching.
    pub fn retire_thread(&mut self, runtime_id: u64) {
        if let Some(address) = self.reverse.remove(&runtime_id) {
            self.bindings.remove(&address);
        }
    }
    fn match_thread(
        &mut self,
        expected: &Option<CapturedThread>,
        actual: &Option<ThreadContext>,
    ) -> Result<()> {
        match (expected, actual) {
            (None, None) => Ok(()),
            (Some(e), Some(a)) => {
                let mut normalized = a.clone();
                normalized.name = normalized.name.as_deref().map(normalize_name);
                normalized.id = 0;
                if normalized != e.context {
                    return Err(err(format!(
                        "thread context mismatch: expected {:?}, got {:?}",
                        e.context, normalized
                    )));
                }
                if self.bindings.get(&e.address).is_some_and(|id| *id != a.id)
                    || self
                        .reverse
                        .get(&a.id)
                        .is_some_and(|address| *address != e.address)
                {
                    return Err(err("thread identity binding mismatch"));
                }
                self.bindings.insert(e.address, a.id);
                self.reverse.insert(a.id, e.address);
                Ok(())
            }
            _ => Err(err("thread context presence mismatch")),
        }
    }
    fn try_read(
        &mut self,
        site: ClockSite,
        context: &RequestContext,
        precision: Precision,
        rounding: Rounding,
    ) -> Result<X87> {
        if !self.selected {
            return Err(err("select replay segment before reading"));
        }
        if self.position >= self.accepted {
            return Err(err("accepted clock tape exhausted"));
        }
        let sample = self
            .samples
            .get(self.position)
            .ok_or_else(|| err("clock tape exhausted"))?
            .clone();
        if sample.site != site || site.original_caller() != Some(sample.caller) {
            return Err(err(format!(
                "clock request {} expected {:?} caller {:#x}, got {site:?}",
                sample.ordinal, sample.site, sample.caller
            )));
        }
        let pc = match (sample.cw >> 8) & 3 {
            0 => Precision::F32,
            2 => Precision::F64,
            3 => Precision::Ext,
            _ => return Err(err("reserved original precision")),
        };
        if precision != pc || rounding as u16 != (sample.cw >> 10) & 3 {
            return Err(err(format!(
                "clock FPU control mismatch at request {}: expected {:?}/rounding {}, got {:?}/{:?}",
                sample.ordinal, pc, (sample.cw >> 10) & 3, precision, rounding
            )));
        }
        self.match_thread(&sample.thread, &context.thread)?;
        self.match_thread(&sample.cursor, &context.driver_cursor)?;
        let clock = self
            .clock
            .as_mut()
            .ok_or_else(|| err("clock state absent"))?;
        if clock.state().start != sample.baseline {
            return Err(err("counter baseline changed"));
        }
        struct Inputs {
            frequency: i64,
            counter: i64,
        }
        impl PerformanceCounters for Inputs {
            fn query_frequency(&mut self, out: &mut i64) -> bool {
                *out = self.frequency;
                true
            }
            fn query_counter(&mut self, out: &mut i64) -> bool {
                *out = self.counter;
                true
            }
        }
        let result = clock.read_milliseconds(
            &mut Inputs {
                frequency: sample.frequency,
                counter: sample.counter,
            },
            precision,
            rounding,
        );
        // These caller-owned initializer threads are destroyed after a terminal
        // run. The original reuses their native stack slots for later threads;
        // an address is not a lifetime identity after that proven boundary.
        if site == ClockSite::ThreadExit {
            if let Some(thread) = sample.thread.filter(|t| t.terminal_initializer) {
                if let Some(id) = self.bindings.remove(&thread.address) {
                    self.reverse.remove(&id);
                }
            }
        }
        self.position += 1;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sample(site: ClockSite, ordinal: u64, counter: i64) -> Sample {
        Sample {
            ordinal,
            caller: site.original_caller().unwrap(),
            site,
            entry: ordinal * 10,
            completed: ordinal * 10 + 1,
            frequency: 1000,
            counter,
            baseline: 10,
            cw: 0x3f,
            thread: None,
            cursor: None,
        }
    }
    fn replay(samples: Vec<Sample>) -> Replay {
        let accepted = samples.len();
        Replay {
            samples,
            position: 0,
            after_game_init: 0,
            segment_start: 0,
            accepted,
            selected: false,
            clock: None,
            bindings: HashMap::new(),
            reverse: HashMap::new(),
            failure: None,
            source_facts_sha256: String::new(),
        }
    }
    #[test]
    fn computes_from_counter_inputs_and_rejects_exhaustion() {
        let mut replay = replay(vec![sample(ClockSite::FramePhysicsEpoch, 1, 1000)]);
        replay.select_after_game_init().unwrap();
        let actual = replay
            .read(
                ClockSite::FramePhysicsEpoch,
                &RequestContext::default(),
                Precision::F32,
                Rounding::NearestEven,
            )
            .unwrap();
        assert_eq!(actual, X87::from_i64(990));
        replay.finish_accepted().unwrap();
        assert!(replay
            .read(
                ClockSite::FramePhysicsEpoch,
                &RequestContext::default(),
                Precision::F32,
                Rounding::NearestEven
            )
            .unwrap_err()
            .0
            .contains("exhausted"));
    }
    #[test]
    fn wrong_role_or_context_poison_without_consumption() {
        let mut replay = replay(vec![sample(ClockSite::FramePhysicsEpoch, 1, 1000)]);
        replay.select_after_game_init().unwrap();
        let first = replay
            .read(
                ClockSite::ThreadEnter,
                &RequestContext::default(),
                Precision::F32,
                Rounding::NearestEven,
            )
            .unwrap_err();
        assert_eq!(replay.consumed(), 0);
        assert_eq!(
            replay
                .read(
                    ClockSite::FramePhysicsEpoch,
                    &RequestContext::default(),
                    Precision::F32,
                    Rounding::NearestEven
                )
                .unwrap_err(),
            first
        );
        let mut replay = super::tests::replay(vec![sample(ClockSite::FramePhysicsEpoch, 1, 1000)]);
        replay.select_after_game_init().unwrap();
        let context = RequestContext {
            thread: Some(ThreadContext {
                id: 1,
                name: None,
                class: None,
                method: None,
                node_index: None,
            }),
            driver_cursor: None,
        };
        assert!(replay
            .read(
                ClockSite::FramePhysicsEpoch,
                &context,
                Precision::F32,
                Rounding::NearestEven
            )
            .is_err());
        assert_eq!(replay.consumed(), 0);
    }
    #[test]
    fn wrong_arithmetic_controls_are_not_replaced_by_capture_controls() {
        let mut replay = replay(vec![sample(ClockSite::FramePhysicsEpoch, 1, 1000)]);
        replay.select_after_game_init().unwrap();
        assert!(replay
            .read(
                ClockSite::FramePhysicsEpoch,
                &RequestContext::default(),
                Precision::F64,
                Rounding::NearestEven
            )
            .is_err());
        assert_eq!(replay.consumed(), 0);
    }
    #[test]
    fn terminal_initializer_stack_slot_can_bind_a_later_thread_lifetime() {
        let expected = |terminal| CapturedThread {
            address: 42,
            terminal_initializer: terminal,
            context: ThreadContext {
                id: 0,
                name: Some("THRD-INSTVAR-INI".into()),
                class: None,
                method: None,
                node_index: None,
            },
        };
        let mut first = sample(ClockSite::ThreadEnter, 1, 1000);
        first.thread = Some(expected(false));
        let mut exit = sample(ClockSite::ThreadExit, 2, 1010);
        exit.thread = Some(expected(true));
        let mut second = sample(ClockSite::ThreadEnter, 3, 1020);
        second.thread = Some(expected(false));
        let mut replay = replay(vec![first, exit, second]);
        replay.select_after_game_init().unwrap();
        let context = |id| RequestContext {
            thread: Some(ThreadContext {
                id,
                name: Some("THRD-INSTVAR-INI".into()),
                class: None,
                method: None,
                node_index: None,
            }),
            driver_cursor: None,
        };
        replay
            .read(
                ClockSite::ThreadEnter,
                &context(1),
                Precision::F32,
                Rounding::NearestEven,
            )
            .unwrap();
        replay
            .read(
                ClockSite::ThreadExit,
                &context(1),
                Precision::F32,
                Rounding::NearestEven,
            )
            .unwrap();
        replay
            .read(
                ClockSite::ThreadEnter,
                &context(2),
                Precision::F32,
                Rounding::NearestEven,
            )
            .unwrap();
        replay.finish_accepted().unwrap();
    }
    #[test]
    fn explicit_thread_destruction_releases_address_lifetime_binding() {
        let mut replay = replay(vec![]);
        let expected = Some(CapturedThread {
            address: 42,
            terminal_initializer: false,
            context: ThreadContext {
                id: 0,
                name: Some("THRD-RUNVMI Config.getInfo".into()),
                class: None,
                method: None,
                node_index: None,
            },
        });
        let actual = |id| {
            Some(ThreadContext {
                id,
                name: Some("THRD-RUNVMI Config.getInfo".into()),
                class: None,
                method: None,
                node_index: None,
            })
        };
        replay.match_thread(&expected, &actual(1)).unwrap();
        assert!(replay.match_thread(&expected, &actual(2)).is_err());
        replay.retire_thread(1);
        replay.match_thread(&expected, &actual(2)).unwrap();
    }
    #[test]
    fn driver_destruction_releases_address_for_next_run_in_the_same_drive() {
        use slr_vm::{interp::flags, sched::Scheduler, threads::ThreadQueues};
        use std::cell::RefCell;
        let replay = RefCell::new(replay(vec![]));
        let expected = Some(CapturedThread {
            address: 42,
            terminal_initializer: false,
            context: ThreadContext {
                id: 0,
                name: Some("worker".into()),
                class: None,
                method: None,
                node_index: None,
            },
        });
        let mut queues = ThreadQueues::new();
        let first = queues.spawn(10, 0, "worker");
        let second = queues.spawn(10, 0, "worker");
        let mut run = |queues: &mut ThreadQueues, id, _, _: &mut dyn FnMut() -> X87| {
            let actual = Some(ThreadContext {
                id: id as u64 + 1,
                name: Some("worker".into()),
                class: None,
                method: None,
                node_index: None,
            });
            replay
                .borrow_mut()
                .match_thread(&expected, &actual)
                .unwrap();
            queues.get_mut(id).flags = flags::STOPPED;
        };
        let mut reap = |queues: &mut ThreadQueues, id| {
            replay.borrow_mut().retire_thread(id as u64 + 1);
            queues.remove(id);
        };
        let mut scheduler = Scheduler::new();
        scheduler.drive_with_reaper(
            &mut queues,
            0.0,
            0,
            &mut || X87::from_i64(0),
            &mut run,
            &mut reap,
        );
        assert_eq!(scheduler.reaped(), &[first, second]);
        assert!(replay.borrow().bindings.is_empty());
        assert!(replay.borrow().reverse.is_empty());
    }

    #[test]
    fn names_only_normalize_address_suffixes() {
        assert_eq!(
            normalize_name("Ljava.render.LoadingScreen;<202acf80>"),
            "Ljava.render.LoadingScreen;"
        );
        assert_eq!(normalize_name("Hotkey watcher"), "Hotkey watcher");
        assert_eq!(normalize_name("user<face>"), "user<face>");
        assert_eq!(
            normalize_name("thread<not-an-address>"),
            "thread<not-an-address>"
        );
    }
    #[test]
    fn incomplete_capture_still_fails_strict_runtime_loaders() {
        let temp =
            std::env::temp_dir().join(format!("slr-clock-incomplete-{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let tape = temp.join("clock-inputs.jsonl");
        assert!(Replay::load(&tape).is_err());
        assert!(crate::frame_timer::FrameTimerReplay::load_companion(&tape).is_err());
        fs::write(temp.join("clock-verification.json"), b"corrupt manifest").unwrap();
        assert!(Replay::load(&tape).is_err());
        fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn parser_rejects_role_caller_disagreement_and_hash_tampering() {
        assert!(Sample::parse(&json!({"caller":0x428a5b,"role":"ThreadEnter"})).is_err());

        let temp =
            std::env::temp_dir().join(format!("slr-clock-replay-hash-{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        // Synthetic provenance is sufficient: the loader must reject the
        // digest before it could read passive source facts or clock results.
        fs::write(
            temp.join("clock-verification.json"),
            serde_json::to_vec(&json!({
                "truncated":false,"pending_clocks":0,"context_errors":[],
                "events":1,"event_limit":1,
                "files":{"clock-inputs.jsonl":{"sha256":digest(b"untampered input"),"bytes":b"untampered input".len()}}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(temp.join("clock-inputs.jsonl"), b"changed capture").unwrap();
        let result = Replay::load(temp.join("clock-inputs.jsonl"));
        fs::remove_dir_all(&temp).unwrap();
        assert!(result.err().unwrap().0.contains("SHA256"));
    }
    #[test]
    fn earliest_capture_boundary_recomputes_all_original_clock_inputs() {
        let Some(base) = crate::oracle_fixture::clock_capture() else {
            return;
        };
        let mut replay = Replay::load(base.join("clock-inputs.jsonl"))
            .expect("local original replay capture required");
        let selected = replay.select_start().unwrap();
        assert_eq!(selected.next_request_ordinal, 1);
        assert_eq!(selected.baseline, 26566);
        assert_eq!(selected.accepted_remaining, 18926);
        assert!(replay.check_after_game_init().is_err());
        let references = fs::read_to_string(base.join("clock-reference-results.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>();
        while replay.position < replay.accepted {
            let sample = replay.samples[replay.position].clone();
            let convert = |t: &CapturedThread| {
                let mut c = t.context.clone();
                c.id = t.address;
                c
            };
            let context = RequestContext {
                thread: sample.thread.as_ref().map(convert),
                driver_cursor: sample.cursor.as_ref().map(convert),
            };
            if replay.position == replay.after_game_init {
                replay.check_after_game_init().unwrap();
            }
            // This arithmetic replay test supplies the recorded FPU controls;
            // live callers must independently provide their actual controls.
            let precision = match (sample.cw >> 8) & 3 {
                0 => Precision::F32,
                2 => Precision::F64,
                3 => Precision::Ext,
                _ => panic!("invalid recorded precision"),
            };
            let output = replay
                .read(sample.site, &context, precision, Rounding::NearestEven)
                .unwrap();
            let hex = output
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert_eq!(
                json!(hex),
                references[sample.ordinal as usize - 1]["return_st0_le"],
                "request {}",
                sample.ordinal
            );
        }
        replay.finish_accepted().unwrap();
    }
    #[test]
    fn original_capture_loads_and_counter_arithmetic_matches_reference_through_menu() {
        let Some(base) = crate::oracle_fixture::clock_capture() else {
            return;
        };
        let mut replay = Replay::load(base.join("clock-inputs.jsonl"))
            .expect("local original replay capture required");
        let selected = replay.select_after_game_init().unwrap();
        assert_eq!(selected.next_request_ordinal, 6208);
        assert_eq!(selected.baseline, 26566);
        assert_eq!(selected.accepted_remaining, 18926 - 6207);
        let references = fs::read_to_string(base.join("clock-reference-results.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>();
        while replay.position < replay.accepted {
            let sample = replay.samples[replay.position].clone();
            let convert = |t: &CapturedThread| {
                let mut c = t.context.clone();
                c.id = t.address;
                c
            };
            let context = RequestContext {
                thread: sample.thread.as_ref().map(convert),
                driver_cursor: sample.cursor.as_ref().map(convert),
            };
            let output = replay
                .read(sample.site, &context, Precision::F32, Rounding::NearestEven)
                .unwrap();
            let hex = output
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert_eq!(
                json!(hex),
                references[sample.ordinal as usize - 1]["return_st0_le"],
                "request {}",
                sample.ordinal
            );
        }
        replay.finish_accepted().unwrap();
    }
}
