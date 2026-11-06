//! What the app's exit code tells the supervisor.
pub const OK: i32 = 0;
/// `fail()`: the app could not start and restarting would not help.
pub const CONFIG: i32 = 2;
/// Another copy was running and has been brought forward.
pub const ALREADY_RUNNING: i32 = 3;
/// Another program holds the port.
pub const PORT_TAKEN: i32 = 4;
/// The supervisor's own exit, after three crashes within 60 s.
pub const GAVE_UP: i32 = 5;
pub const PANIC: i32 = 101;
