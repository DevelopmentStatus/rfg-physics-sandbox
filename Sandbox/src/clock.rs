//! Simulation run-state: tick counter, pause, and single-step. Plain Rust with
//! no Bevy dependency so the control rules can be tested headless. The app
//! applies this state to Bevy's fixed-step time (see `main.rs`).

/// Simulation step in seconds (60 Hz).
pub const TICK_SECONDS: f64 = 1.0 / 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TogglePause,
    Pause,
    Resume,
    /// Pauses if running, then advances exactly one tick.
    Step,
    /// Sets the tick counter back to zero. The pause state is kept.
    Reset,
}

#[derive(Debug, Default)]
pub struct SimClock {
    tick: u64,
    paused: bool,
    step_pending: bool,
}

impl SimClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::TogglePause => self.paused = !self.paused,
            Command::Pause => self.paused = true,
            Command::Resume => self.paused = false,
            Command::Step => {
                self.paused = true;
                self.step_pending = true;
            }
            Command::Reset => {
                self.tick = 0;
                self.step_pending = false;
            }
        }
    }

    /// Returns true once per `Step` command; the caller must then run exactly
    /// one fixed tick.
    pub fn take_step(&mut self) -> bool {
        std::mem::take(&mut self.step_pending)
    }

    /// Called once per executed fixed tick, whether free-running or stepped.
    pub fn count_tick(&mut self) {
        self.tick += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_is_requested_exactly_once_and_pauses() {
        let mut clock = SimClock::new();
        clock.apply(Command::Step);
        assert!(clock.is_paused());
        assert!(clock.take_step());
        assert!(!clock.take_step());
    }

    #[test]
    fn step_while_running_pauses() {
        let mut clock = SimClock::new();
        clock.apply(Command::Resume);
        clock.apply(Command::Step);
        assert!(clock.is_paused());
    }

    #[test]
    fn toggle_pause_flips_state() {
        let mut clock = SimClock::new();
        clock.apply(Command::TogglePause);
        assert!(clock.is_paused());
        clock.apply(Command::TogglePause);
        assert!(!clock.is_paused());
    }

    #[test]
    fn reset_zeroes_tick_and_keeps_pause_state() {
        let mut clock = SimClock::new();
        clock.count_tick();
        clock.count_tick();
        clock.apply(Command::Reset);
        assert_eq!(clock.tick(), 0);
        assert!(!clock.is_paused());

        clock.apply(Command::Pause);
        clock.apply(Command::Reset);
        assert!(clock.is_paused());
    }

    #[test]
    fn reset_cancels_pending_step() {
        let mut clock = SimClock::new();
        clock.apply(Command::Step);
        clock.apply(Command::Reset);
        assert!(!clock.take_step());
        assert_eq!(clock.tick(), 0);
    }

    #[test]
    fn paused_clock_counts_nothing_until_ticked() {
        let mut clock = SimClock::new();
        clock.apply(Command::Pause);
        assert_eq!(clock.tick(), 0);
        clock.count_tick();
        assert_eq!(clock.tick(), 1);
    }
}
