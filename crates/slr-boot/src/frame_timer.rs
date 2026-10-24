//! Strict replay of external QPC inputs to original Timer7 frame-end samples.
//! The tape contains no elapsed-time result. Original552330 arithmetic and the
//! following FST m32 run here using the caller's actual arithmetic controls.
use serde_json::Value;
use sha2::{Digest, Sha256};
use slr_x87::{Precision, Rounding, X87};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
struct Input {
    frame: u64,
    counter: u64,
    start: u64,
    frequency: u64,
    cw: u16,
}

fn number(row: &Value, key: &str) -> Result<u64, String> {
    row[key]
        .as_u64()
        .ok_or_else(|| format!("missing or invalid frame-timer {key}"))
}

fn elapsed(input: Input, precision: Precision, rounding: Rounding) -> Result<f32, String> {
    let captured_precision = match (input.cw >> 8) & 3 {
        0 => Precision::F32,
        2 => Precision::F64,
        3 => Precision::Ext,
        _ => return Err("reserved frame-timer precision".into()),
    };
    if precision != captured_precision || rounding as u16 != (input.cw >> 10) & 3 {
        return Err("frame-timer arithmetic control mismatch".into());
    }
    if input.frequency == 0 {
        return Err("zero frame-timer frequency".into());
    }
    // FILD m64 loads exactly, then FSUBP/FDIVP round under the real controls.
    let delta = X87::from_i64(input.counter as i64).sub(
        X87::from_i64(input.start as i64),
        precision,
        rounding,
    );
    Ok(delta
        .div(X87::from_i64(input.frequency as i64), precision, rounding)
        .to_f32(precision, rounding))
}

pub struct FrameTimerReplay {
    inputs: Vec<Input>,
    cursor: usize,
    failure: Option<String>,
}

impl FrameTimerReplay {
    /// The embedding obtains the expected digest from its capture manifest.
    pub fn load(path: &Path, expected_sha256: &str) -> Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err("frame-timer tape exceeds16MiB".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != expected_sha256 {
            return Err("frame-timer tape SHA256 mismatch".into());
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        Self::parse(text)
    }

    /// Load the timer companion tied to the same immutable CounterClock input
    /// tape and passive source, rather than accepting an unverified timer file.
    pub fn load_companion(clock_inputs: &Path) -> Result<(Self, usize), String> {
        let base = clock_inputs.parent().ok_or("clock tape has no parent")?;
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(base.join("frame-timer-manifest.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        if manifest["clock_inputs_sha256"].as_str()
            != Some(hash(&std::fs::read(clock_inputs).map_err(|e| e.to_string())?).as_str())
        {
            return Err("frame timer companion references another clock tape".into());
        }
        if manifest["pending"] != 0
            || manifest["truncated"] != false
            || manifest["errors"].as_array().is_none_or(|v| !v.is_empty())
        {
            return Err("incomplete frame timer companion".into());
        }
        let source = base.join(
            manifest["source_facts"]
                .as_str()
                .ok_or("missing timer source facts")?,
        );
        let metadata = std::fs::metadata(&source).map_err(|e| e.to_string())?;
        if metadata.len() > 64 * 1024 * 1024 {
            return Err("timer source exceeds64MiB".into());
        }
        let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
        if manifest["source_facts_sha256"].as_str() != Some(hash(&bytes).as_str()) {
            return Err("timer source SHA256 mismatch".into());
        }
        let source: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let history = &source["frame_timer_history"];
        if history["alters_execution"] != false
            || history["raw_inputs_only"] != true
            || history["truncated"] != false
            || history["pending"] != 0
            || history["errors"].as_array().is_none_or(|v| !v.is_empty())
        {
            return Err("invalid passive frame timer source".into());
        }
        let paired: Value = serde_json::from_slice(
            &std::fs::read(base.join("observed/original-facts.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if source["clock_history"]["events"] != paired["clock_history"]["events"] {
            return Err("frame timer source clock sequence differs".into());
        }
        let timer = Self::load(
            &base.join(manifest["file"].as_str().ok_or("missing timer file")?),
            manifest["sha256"].as_str().ok_or("missing timer SHA256")?,
        )?;
        if timer.available() as u64 != number(&manifest, "rows")?
            || history["rows"] != manifest["rows"]
        {
            return Err("timer source row count mismatch".into());
        }
        let resets = history["resets"]
            .as_array()
            .ok_or("missing timer reset inputs")?;
        for row in &timer.inputs {
            let reset = resets
                .get(row.frame as usize - 1)
                .ok_or("missing timer reset")?;
            if number(reset, "frame")? != row.frame
                || number(reset, "start")? != row.start
                || number(reset, "frequency")? != row.frequency
            {
                return Err("timer reset/source mismatch".into());
            }
        }
        let accepted =
            usize::try_from(number(&manifest, "accepted_frames")?).map_err(|e| e.to_string())?;
        if accepted == 0 || accepted > timer.available() {
            return Err("invalid accepted timer frame count".into());
        }
        Ok((timer, accepted))
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut inputs = Vec::new();
        for line in text.lines() {
            let row: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let map = row.as_object().ok_or("frame-timer row must be an object")?;
            if map.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "frame"
                        | "counter"
                        | "start"
                        | "frequency"
                        | "cw"
                        | "caller"
                        | "entry_icount"
                        | "completed_icount"
                )
            }) {
                return Err(
                    "unexpected frame-timer field; only raw inputs and provenance are accepted"
                        .into(),
                );
            }
            let frame = number(&row, "frame")?;
            if frame != inputs.len() as u64 + 1 || number(&row, "caller")? != 0x428ED0 {
                return Err("frame-timer sequence or caller mismatch".into());
            }
            if number(&row, "completed_icount")? < number(&row, "entry_icount")? {
                return Err("frame-timer observation order mismatch".into());
            }
            let frequency = number(&row, "frequency")?;
            if frequency == 0 {
                return Err("zero frame-timer frequency".into());
            }
            inputs.push(Input {
                frame,
                counter: number(&row, "counter")?,
                start: number(&row, "start")?,
                frequency,
                cw: u16::try_from(number(&row, "cw")?).map_err(|_| "frame-timer CW overflow")?,
            });
        }
        Ok(Self {
            inputs,
            cursor: 0,
            failure: None,
        })
    }

    /// Frames are one-based; a mismatch poisons the tape without consuming it.
    pub fn next(
        &mut self,
        frame: u64,
        precision: Precision,
        rounding: Rounding,
    ) -> Result<f32, String> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            let input = *self
                .inputs
                .get(self.cursor)
                .ok_or("frame-timer tape exhausted")?;
            if input.frame != frame {
                return Err(format!(
                    "frame-timer request {frame}, expected{}",
                    input.frame
                ));
            }
            elapsed(input, precision, rounding)
        })();
        match result {
            Ok(value) => {
                self.cursor += 1;
                Ok(value)
            }
            Err(error) => {
                self.failure = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn consumed(&self) -> usize {
        self.cursor
    }
    pub fn available(&self) -> usize {
        self.inputs.len()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn paired_passive_capture_loads_with_validated_clock_and_reset_inputs() {
        let Some(base) = crate::oracle_fixture::clock_capture() else {
            return;
        };
        let path = base.join("clock-inputs.jsonl");
        let (mut timer, accepted) = super::FrameTimerReplay::load_companion(&path).unwrap();
        assert_eq!(timer.available(), 352);
        assert_eq!(accepted, 331);
        let first = timer
            .next(1, slr_x87::Precision::F32, slr_x87::Rounding::NearestEven)
            .unwrap();
        assert!(first > 0.0);
    }
    use super::*;
    const ROW: &str = r#"{"frame":1,"counter":1500,"start":1000,"frequency":10000,"cw":639,"caller":4361936,"entry_icount":1,"completed_icount":2}"#;

    #[test]
    fn raw_inputs_recompute_original_timer_projection() {
        let mut tape = FrameTimerReplay::parse(ROW).unwrap();
        // test_frame_timer_history executes the actual caller552330 + FST m32
        // for these declared external values, with and without passive hooks.
        assert_eq!(
            tape.next(1, Precision::F64, Rounding::NearestEven)
                .unwrap()
                .to_bits(),
            0.05f32.to_bits()
        );
        assert_eq!(tape.consumed(), 1);
        assert!(tape.next(2, Precision::F64, Rounding::NearestEven).is_err());
    }

    #[test]
    fn mismatches_and_result_tapes_fail_closed() {
        let mut tape = FrameTimerReplay::parse(ROW).unwrap();
        assert!(tape.next(1, Precision::F32, Rounding::NearestEven).is_err());
        assert!(tape.next(1, Precision::F64, Rounding::NearestEven).is_err());
        assert_eq!(tape.consumed(), 0);
        assert!(FrameTimerReplay::parse(
            &ROW.replace("\"counter\":1500", "\"return_st0_le\":\"00\"")
        )
        .is_err());
        assert!(FrameTimerReplay::parse(&ROW.replace("\"frame\":1", "\"frame\":2")).is_err());
    }
}
