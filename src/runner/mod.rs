pub mod plugin;
pub mod types;
pub mod python;
pub mod discovery;
pub mod execution;

pub use plugin::{OxtestPlugin, register_plugin};
pub use types::{CaptureMode, FixtureUsage, RunConfig, TestItem, TestResult, TestSummary};
pub use discovery::{discover_tests, list_fixtures, list_fixtures_per_test, list_markers};
pub use execution::run_tests;
