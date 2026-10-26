//! Consume a validated live counter input through the actual5516c0 Win32 CALLs.
//! The packet contains numeric inputs, not clock answers. The source clock
//! must already own its baseline and control state; this provider never seeds
//! missing machine history or samples the replay a second time.
use crate::{native_stack as stack, native_vm_fpu::State};
use slr_physics::space::Space;
use slr_vm::clock::{ClockSite, CounterInput};

pub struct Counter {
    input: CounterInput,
    caller: u32,
    phase: u8,
    failure: Option<String>,
}
impl Counter {
    pub fn new(
        m: &Space,
        fpu: &State,
        site: ClockSite,
        input: CounterInput,
    ) -> Result<Self, String> {
        let caller = site
            .original_caller()
            .ok_or("NativeVmCounter: unspecified source clock caller")?;
        // x87 normalizes reserved CW bit6, while Unicorn retains its input
        // representation. The independent clock referee verifies this bit.
        if (fpu.control() ^ input.source_control) & !0x40 != 0 {
            return Err("NativeVmCounter: live FPU control differs from issued input".into());
        }
        let baseline = u64::from(m.r32(0x76a288)) | (u64::from(m.r32(0x76a28c)) << 32);
        if baseline as i64 != input.baseline {
            return Err("NativeVmCounter: live clock baseline is not established".into());
        }
        Ok(Self {
            input,
            caller,
            phase: 0,
            failure: None,
        })
    }
    fn query(&mut self, m: &mut Space, destination: u32, frequency: bool) -> Result<u32, String> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.try_query(m, destination, frequency);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }
    fn try_query(
        &mut self,
        m: &mut Space,
        destination: u32,
        frequency: bool,
    ) -> Result<u32, String> {
        let expected_phase = if frequency { 0 } else { 1 };
        let expected_destination = if frequency { 0x76a280 } else { 0x76a290 };
        let expected_return = if frequency { 0x5516cb } else { 0x5516d6 };
        let sp = stack::sp(m);
        if self.phase != expected_phase
            || destination != expected_destination
            || !stack::active(m)
            || m.r32(sp) != expected_return
            || m.r32(sp + 4) != destination
            || m.r32(sp + 8) != self.caller
        {
            return Err(format!(
                "NativeVmCounter: input {} does not match the current source counter CALL",
                self.input.ordinal
            ));
        }
        let value = if frequency {
            self.input.frequency
        } else {
            self.input.counter
        } as u64;
        m.w32(destination, value as u32);
        m.w32(destination + 4, (value >> 32) as u32);
        self.phase += 1;
        Ok(1)
    }
    pub fn frequency(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
        self.query(m, destination, true)
    }
    pub fn counter(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
        self.query(m, destination, false)
    }
    pub fn finish(self) -> Result<(), String> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.phase != 2 {
            return Err(
                "NativeVmCounter: source clock did not consume both numeric queries".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Env(Counter);
    impl crate::native_vm_pool::Allocator for Env {
        fn allocate(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
            Err("clock must not allocate".into())
        }
        fn reallocate(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<u32, String> {
            Err("clock must not reallocate".into())
        }
    }
    impl crate::native_vm_dispatch::Environment for Env {
        fn free(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            Err("clock must not free".into())
        }
    }
    impl crate::native_vm_lookup::Environment for Env {}
    impl crate::native_vm_thread_run::Environment for Env {
        fn query_frequency(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
            self.0.frequency(m, p)
        }
        fn query_counter(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
            self.0.counter(m, p)
        }
    }
    fn scenario() -> (Space, State, CounterInput) {
        let mut m = Space::new();
        m.w32(0x76a288, 1000000);
        m.w32(0x5f0910, 1000f32.to_bits());
        let mut fpu = State::finit([slr_x87::X87::from_i64(0); 8]);
        fpu.load_control(0x27f);
        (
            m,
            fpu,
            CounterInput {
                ordinal: 7,
                frequency: 1024,
                counter: 1000512,
                baseline: 1000000,
                source_control: 0x23f,
            },
        )
    }
    #[test]
    fn original_source_clock_consumes_numeric_inputs_once_at_its_actual_call_site() {
        let (mut m, mut fpu, input) = scenario();
        let mut env = Env(Counter::new(&m, &fpu, ClockSite::ThreadEnter, input).unwrap());
        let parent = stack::enter(&mut m, &[], None);
        stack::call(&mut m, 0x421066, &[], None, |m| {
            crate::native_vm_thread_run::clock(m, &mut fpu, &mut env)
        })
        .unwrap();
        env.0.finish().unwrap();
        assert_eq!(fpu.st(0).unwrap(), slr_x87::X87::from_i64(500));
        assert_eq!(m.r32(0x76a280), 1024);
        assert_eq!(m.r32(0x76a290), 1000512);
        assert_eq!(m.r32(0x76a288), 1000000);
        stack::leave(&mut m, parent);
    }
    #[test]
    fn missing_baseline_and_wrong_control_fail_without_seeding_history() {
        let (mut m, mut fpu, input) = scenario();
        m.w32(0x76a288, 0);
        m.begin_journal();
        assert!(Counter::new(&m, &fpu, ClockSite::ThreadEnter, input).is_err());
        assert!(m.end_journal().is_empty());
        assert_eq!(m.r32(0x76a288), 0);
        m.w32(0x76a288, 1000000);
        fpu.load_control(0x7f);
        m.begin_journal();
        assert!(Counter::new(&m, &fpu, ClockSite::ThreadEnter, input).is_err());
        assert!(m.end_journal().is_empty());
    }
    #[test]
    fn wrong_caller_poisons_the_packet_without_returning_another_sample() {
        let (mut m, mut fpu, input) = scenario();
        m.w32(0x76a280, 0x12345678);
        let mut env = Env(Counter::new(&m, &fpu, ClockSite::ThreadEnter, input).unwrap());
        let parent = stack::enter(&mut m, &[], None);
        let first = stack::call(&mut m, 0x47bfe5, &[], None, |m| {
            crate::native_vm_thread_run::clock(m, &mut fpu, &mut env)
        })
        .unwrap_err();
        assert_eq!(m.r32(0x76a280), 0x12345678);
        assert_eq!(m.r32(0x76a290), 0);
        let second = stack::call(&mut m, 0x421066, &[], None, |m| {
            crate::native_vm_thread_run::clock(m, &mut fpu, &mut env)
        })
        .unwrap_err();
        assert_eq!(first, second);
        assert_eq!(env.0.finish().unwrap_err(), first);
        assert_eq!(m.r32(0x76a280), 0x12345678);
        stack::leave(&mut m, parent);
    }
}
