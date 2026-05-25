"""Allow running the installed cobratest executable via `python -m cobratest`."""

from __future__ import annotations

import os
import subprocess
import sys
import sysconfig
from pathlib import Path


def _binary_name() -> str:
    return "cobratest.exe" if os.name == "nt" else "cobratest"


def _binary_path() -> Path:
    scripts_dir = sysconfig.get_path("scripts")
    if not scripts_dir:
        raise RuntimeError("Unable to locate the Python scripts directory")

    binary = Path(scripts_dir) / _binary_name()
    if not binary.exists():
        raise FileNotFoundError(f"Installed cobratest executable not found at {binary}")
    return binary


def main() -> int:
    binary = _binary_path()
    completed = subprocess.run([str(binary), *sys.argv[1:]], check=False)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
