# oxtest

`oxtest` is a Rust-based Python test runner scaffold using `PyO3` to discover and execute Python tests.

## Install with pip

Create and activate a virtual environment, then install from the repository root:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install .
```

After installation, the `oxtest` command will be available in the virtual environment:

```bash
oxtest --help
```

The install also provides an importable Python package and module entrypoint:

```bash
python -c "import oxtest; print(oxtest.__version__)"
python -m oxtest --help
```


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
