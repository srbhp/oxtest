use clap::ValueEnum;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, ValueEnum)]
pub enum CaptureMode {
    Fd,
    Sys,
    No,
    TeeSys,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunConfig {
    pub k_expr: Option<String>,
    pub m_expr: Option<String>,
    pub exitfirst: bool,
    pub maxfail: Option<usize>,
    pub jobs: usize,
    pub ignore: Vec<PathBuf>,
    pub ignore_glob: Vec<String>,
    pub capture: CaptureMode,
    pub collect_only: bool,
    pub quiet: bool,
    pub verbose: u8,
    pub strict: bool,
    pub strict_markers: bool,
    pub strict_config: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TestItem {
    pub file: PathBuf,
    pub name: String,
    pub full_name: String,
    pub marks: Vec<String>,
    pub extra_keywords: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TestResult {
    pub file: PathBuf,
    pub full_name: String,
    pub passed: bool,
    pub output: String,
    pub marks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TestSummary {
    pub results: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct FixtureUsage {
    pub test: String,
    pub fixtures: Vec<String>,
}

impl TestSummary {
    pub fn print_summary(&self) {
        for result in &self.results {
            if result.passed {
                println!("ok {}:{}", result.file.display(), result.full_name);
            } else {
                println!("FAILED {}:{}", result.file.display(), result.full_name);
                println!("{}
", result.output.trim_end());
            }
        }

        println!("
Summary: {} passed, {} failed", self.passed, self.failed);
    }
}
