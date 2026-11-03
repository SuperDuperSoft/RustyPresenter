//! Exit codes shared by the `--app` process and its supervisor (Plan 4 Global Constraints).

/// Normal quit: the supervisor exits too.
pub const OK: i32 = 0;
/// `fail()`: bad set-up the app cannot fix by restarting, so no restart.
pub const CONFIG: i32 = 2;
/// Another copy is already running and was asked to come forward; the supervisor exits 0.
pub const ALREADY_RUNNING: i32 = 3;
/// The port is held by another program: one message, no restart.
pub const PORT_TAKEN: i32 = 4;
/// Only ever the supervisor's own exit: three crashes within 60 s.
pub const GAVE_UP: i32 = 5;
/// A panic on a critical thread (Rust's own panic exit code): restart.
pub const PANIC: i32 = 101;
