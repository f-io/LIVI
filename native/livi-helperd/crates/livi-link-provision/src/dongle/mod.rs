pub mod arm;
pub mod hook;
pub mod lfwb;
pub mod ota;
pub mod probe;
pub mod riscv;
pub mod shell;
pub mod web;

pub const DONGLE_HOST: &str = "192.168.50.100";
pub const BIND_SHELL_PORT: u16 = 2323;
