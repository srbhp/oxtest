use anyhow::Result;
use clap::{Parser, Subcommand};
use oxtest::{
    list_fixtures,
    list_fixtures_per_test,
    list_markers,
    discover_tests,
    run_tests,
    CaptureMode,
    RunConfig,
};
use serde_json::to_string_pretty;

#[derive(Parser)]
#[command(name = "oxtest", version, author, about = "Rust-powered Python test runner using PyO3 and pytest-style CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List discovered Python tests
    List {
        #[arg(value_name = "PATH", default_value = ".")]
        path: String,
        #[arg(short = 'k', long = "keyword", help = "Only run tests which match the given substring expression")]
        keyword: Option<String>,
        #[arg(short = 'm', long = "markexpr", help = "Only run tests matching given mark expression")]
        markexpr: Option<String>,
        #[arg(long, help = "Show available markers")]
        markers: bool,
        #[arg(long, help = "Show available fixtures")]
        fixtures: bool,
        #[arg(long = "fixtures-per-test", help = "Show fixtures used by each test")]
        fixtures_per_test: bool,
        #[arg(long, help = "Output discovered tests as JSON")]
        json: bool,
        #[arg(long = "ignore", help = "Ignore path during collection", value_name = "PATH")]
        ignore: Vec<String>,
        #[arg(long = "ignore-glob", help = "Ignore path pattern during collection", value_name = "PATTERN")]
        ignore_glob: Vec<String>,
    },
    /// Run discovered Python tests
    Run {
        #[arg(value_name = "PATH", default_value = ".")]
        path: String,
        #[arg(short = 'k', long = "keyword", help = "Only run tests which match the given substring expression")]
        keyword: Option<String>,
        #[arg(short = 'm', long = "markexpr", help = "Only run tests matching given mark expression")]
        markexpr: Option<String>,
        #[arg(short = 'j', long = "jobs", help = "Number of workers for parallel execution")]
        jobs: Option<usize>,
        #[arg(short = 'x', long = "exitfirst", help = "Exit instantly on first error or failed test")]
        exitfirst: bool,
        #[arg(long = "maxfail", help = "Exit after first num failures or errors")]
        maxfail: Option<usize>,
        #[arg(long = "strict", help = "Enabled strict option")]
        strict: bool,
        #[arg(long = "strict-markers", help = "Enabled strict-markers option")]
        strict_markers: bool,
        #[arg(long = "strict-config", help = "Enabled strict-config option")]
        strict_config: bool,
        #[arg(long = "capture", help = "Per-test capturing method: fd|sys|no|tee-sys")]
        capture: Option<CaptureMode>,
        #[arg(short = 's', long = "capture-no", help = "Shortcut for --capture=no")]
        capture_no: bool,
        #[arg(long = "collect-only", help = "Only collect tests, don't execute them")]
        collect_only: bool,
        #[arg(long, help = "Show available markers")]
        markers: bool,
        #[arg(long, help = "Show available fixtures")]
        fixtures: bool,
        #[arg(long = "fixtures-per-test", help = "Show fixtures used by each test")]
        fixtures_per_test: bool,
        #[arg(long, help = "Output run results as JSON")]
        json: bool,
        #[arg(long = "ignore", help = "Ignore path during collection", value_name = "PATH")]
        ignore: Vec<String>,
        #[arg(long = "ignore-glob", help = "Ignore path pattern during collection", value_name = "PATTERN")]
        ignore_glob: Vec<String>,
        #[arg(short = 'q', long = "quiet", action = clap::ArgAction::Count, help = "Decrease verbosity")]
        quiet: u8,
        #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, help = "Increase verbosity")]
        verbose: u8,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List {
            path,
            keyword,
            markexpr,
            markers,
            fixtures,
            fixtures_per_test,
            json,
            ignore,
            ignore_glob,
        } => {
            let config = RunConfig {
                k_expr: keyword,
                m_expr: markexpr,
                exitfirst: false,
                maxfail: None,
                jobs: 1,
                ignore: ignore.into_iter().map(std::path::PathBuf::from).collect(),
                ignore_glob,
                capture: CaptureMode::No,
                collect_only: true,
                quiet: false,
                verbose: 0,
                strict: false,
                strict_markers: false,
                strict_config: false,
            };

            if markers {
                let markers = list_markers(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&markers)?);
                } else {
                    for marker in markers {
                        println!("{marker}");
                    }
                }
                return Ok(());
            }

            if fixtures {
                let fixtures = list_fixtures(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&fixtures)?);
                } else {
                    for fixture in fixtures {
                        println!("{fixture}");
                    }
                }
                return Ok(());
            }

            if fixtures_per_test {
                let usages = list_fixtures_per_test(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&usages)?);
                } else {
                    for usage in usages {
                        println!("{}: {}", usage.test, usage.fixtures.join(", "));
                    }
                }
                return Ok(());
            }

            let tests = discover_tests(&path, &config)?;
            if json {
                println!("{}", to_string_pretty(&tests)?);
            } else {
                for test in tests {
                    println!("{}:{}", test.file.display(), test.full_name);
                }
            }
        }
        Commands::Run {
            path,
            keyword,
            markexpr,
            jobs,
            exitfirst,
            maxfail,
            strict,
            strict_markers,
            strict_config,
            capture,
            capture_no,
            collect_only,
            markers,
            fixtures,
            fixtures_per_test,
            json,
            ignore,
            ignore_glob,
            quiet,
            verbose,
        } => {
            let capture_mode = if capture_no {
                CaptureMode::No
            } else {
                capture.unwrap_or(CaptureMode::No)
            };
            let config = RunConfig {
                k_expr: keyword,
                m_expr: markexpr,
                exitfirst,
                maxfail,
                jobs: jobs.unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(usize::from)
                        .unwrap_or(1)
                }),
                ignore: ignore.into_iter().map(std::path::PathBuf::from).collect(),
                ignore_glob,
                capture: capture_mode,
                collect_only,
                quiet: quiet > 0,
                verbose,
                strict,
                strict_markers,
                strict_config,
            };

            if markers {
                let markers = list_markers(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&markers)?);
                } else {
                    for marker in markers {
                        println!("{marker}");
                    }
                }
                return Ok(());
            }

            if fixtures {
                let fixtures = list_fixtures(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&fixtures)?);
                } else {
                    for fixture in fixtures {
                        println!("{fixture}");
                    }
                }
                return Ok(());
            }

            if fixtures_per_test {
                let usages = list_fixtures_per_test(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&usages)?);
                } else {
                    for usage in usages {
                        println!("{}: {}", usage.test, usage.fixtures.join(", "));
                    }
                }
                return Ok(());
            }

            if collect_only {
                let tests = discover_tests(&path, &config)?;
                if json {
                    println!("{}", to_string_pretty(&tests)?);
                } else {
                    for test in tests {
                        println!("{}:{}", test.file.display(), test.full_name);
                    }
                }
                return Ok(());
            }

            let summary = run_tests(&path, config)?;
            if json {
                println!("{}", to_string_pretty(&summary)?);
            } else {
                summary.print_summary();
            }
        }
    }

    Ok(())
}
