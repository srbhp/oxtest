# cobratest

`cobratest` is a Rust-based Python test runner scaffold using `PyO3` to discover and execute Python tests.

## Install with pip

### From PyPI (recommended)

Create and activate a virtual environment, then install from PyPI:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install cobratest
```

### From source (development)

Clone the repository and install from the source directory:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install .
```

### Usage

After installation, the `cobratest` command will be available in the virtual environment:

```bash
cobratest --help
```

The install also provides an importable Python package and module entrypoint:

```bash
python -c "import cobratest; print(cobratest.__version__)"
python -m cobratest --help
```


## Features

- Python AST-based discovery for `def test_*` functions and `Test*` classes
- Support for simple parametrized tests via `@pytest.mark.parametrize`
- Module and class fixtures: `setup_module`, `teardown_module`, `setup_class`, `teardown_class`, `setup_function`, `teardown_function`, `setup_method`, `teardown_method`
- Parallel execution with `rayon`
- CLI and library API support via `cobratest` crate
- JSON output for integration and tooling
- Plugin-style extension API with `cobratest::CobratestPlugin` and `cobratest::register_plugin`

## Testing

Run tests using the `cobratest` command:

```bash
# List all discovered tests
cobratest list .

# Run all tests
cobratest run .

# Run tests in a specific directory
cobratest run ./tests/python

# Run with JSON output for integration
cobratest run . --json
```

For development, use cargo directly:

```bash
# Build and list tests
cargo run -- list .

# Build and run tests
cargo run -- run .
```

## Getting started

1. Install Rust and Python.
2. Run `cargo run -- list .`
3. Run `cargo run -- run .`
