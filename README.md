# oxtest

`oxtest` is a Rust-based Python test runner scaffold using `PyO3` to discover and execute Python tests.

## Commands

- `cargo run -- list .` — list discovered tests in the current directory
- `cargo run -- list . --json` — list discovered tests as JSON
- `cargo run -- run .` — run discovered tests in the current directory
- `cargo run -- run . --jobs 4` — run tests in parallel using 4 workers
- `cargo run -- run . -k 'test_method or test_other'` — run tests matching a keyword expression
- `cargo run -- run . -m 'slow and not network'` — run tests matching a mark expression
- `cargo run -- run . -x --maxfail 1` — stop on the first failure
- `cargo run -- run . --collect-only` — collect tests without executing them
- `cargo run -- run . --markers` — show marker names discovered in the suite
- `cargo run -- run . --fixtures` — show available fixtures discovered in the suite
- `cargo run -- run . --fixtures-per-test` — show fixture usage for each discovered test
- `cargo run -- run . --ignore=build --ignore-glob='**/tests/*'` — ignore files during collection
- `cargo run -- run . --json` — print test results as JSON

## Features

- Python AST-based discovery for `def test_*` functions and `Test*` classes
- Support for simple parametrized tests via `@pytest.mark.parametrize`
- Module and class fixtures: `setup_module`, `teardown_module`, `setup_class`, `teardown_class`, `setup_function`, `teardown_function`, `setup_method`, `teardown_method`
- Parallel execution with `rayon`
- CLI and library API support via `oxtest` crate
- JSON output for integration and tooling
- Plugin-style extension API with `oxtest::OxtestPlugin` and `oxtest::register_plugin`

## Getting started

1. Install Rust and Python.
2. Run `cargo run -- list .`
3. Run `cargo run -- run .`
