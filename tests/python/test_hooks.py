"""Examples of oxtest hook APIs and plugin extension points.

These are lightweight examples meant to document hook usage patterns.
Most hooks are only meaningful when exposed from a plugin module or a
``conftest.py`` file, so this module focuses on importable examples
instead of trying to execute every hook in a live oxtest session.
"""

from __future__ import annotations

from pathlib import Path

import oxtest

hookimpl = getattr(oxtest, "hookimpl", lambda *args, **kwargs: lambda func: func)
hookspec = getattr(oxtest, "hookspec", lambda *args, **kwargs: lambda func: func)


# ---------------------------------------------------------------------------
# @oxtest.hookimpl / @oxtest.hookspec
# ---------------------------------------------------------------------------


class ExampleSpecs:
    """Custom hook specifications for a toy plugin API."""

    @hookspec(firstresult=True)
    def oxtest_example_transform(self, value):
        """Return a transformed value from the first plugin that can handle it."""


class ExamplePlugin:
    """Custom hook implementations matching ExampleSpecs."""

    @hookimpl
    def oxtest_example_transform(self, value):
        if isinstance(value, str):
            return value.upper()
        return None


# ---------------------------------------------------------------------------
# Bootstrapping hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_addhooks(pluginmanager):
    """Register additional hookspecs during oxtest startup."""

    pluginmanager.add_hookspecs(ExampleSpecs)


@hookimpl
def oxtest_plugin_registered(plugin, plugin_name, manager):
    """Observe plugins as they are registered."""

    _ = (plugin, plugin_name, manager)


# ---------------------------------------------------------------------------
# Initialization hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_addoption(parser):
    """Add CLI flags or ini options before collection starts."""

    parser.addoption(
        "--demo-flag",
        action="store_true",
        default=False,
        help="Enable the demo hook examples.",
    )


@hookimpl
def oxtest_configure(config):
    """Perform one-time plugin setup after options are parsed."""

    config.addinivalue_line("markers", "demo: mark tests that belong to hook demos")


@hookimpl
def oxtest_sessionstart(session):
    """Run setup code when the test session begins."""

    _ = session


@hookimpl
def oxtest_sessionfinish(session, exitstatus):
    """Run teardown code at the end of the test session."""

    _ = (session, exitstatus)


# ---------------------------------------------------------------------------
# Collection hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_ignore_collect(collection_path, config):
    """Skip generated folders during collection."""

    _ = config
    return "generated" in str(collection_path)


@hookimpl
def oxtest_collect_file(file_path, parent):
    """Inspect files and decide whether to create a custom collector."""

    _ = parent
    if file_path.suffix == ".hookdemo":
        return None
    return None


@hookimpl
def oxtest_collection_modifyitems(config, items):
    """Reorder or mark collected tests before execution."""

    _ = config
    for item in items:
        if "hooks" in item.name:
            item.add_marker(oxtest.mark.demo)


# ---------------------------------------------------------------------------
# Test running (runtest) hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_runtest_setup(item):
    """Run before each test's setup phase."""

    _ = item


@hookimpl
def oxtest_runtest_call(item):
    """Run immediately before the test function body."""

    _ = item


@hookimpl
def oxtest_runtest_teardown(item, nextitem):
    """Run after each test's teardown phase."""

    _ = (item, nextitem)


@hookimpl(wrapper=True)
def oxtest_runtest_makereport(item, call):
    """Wrap report creation to inspect pass/fail information."""

    outcome = yield
    report = outcome.get_result()
    if report.when == "call" and report.failed:
        item.user_properties.append(("failed_in_demo_hook", True))


# ---------------------------------------------------------------------------
# Reporting hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_report_header(config):
    """Add custom lines near the top of the oxtest report."""

    if config.getoption("--demo-flag"):
        return ["demo-flag enabled for hook examples"]
    return []


@hookimpl
def oxtest_terminal_summary(terminalreporter, exitstatus, config):
    """Emit summary information after the test run completes."""

    _ = (exitstatus, config)
    terminalreporter.write_line("hook demo summary complete")


# ---------------------------------------------------------------------------
# Debugging / interaction hooks
# ---------------------------------------------------------------------------


@hookimpl
def oxtest_exception_interact(node, call, report):
    """Inspect state when an interactive exception is raised."""

    _ = (node, call, report)


@hookimpl
def oxtest_enter_pdb(config, pdb):
    """Run immediately before oxtest drops into pdb."""

    _ = (config, pdb)


@hookimpl
def oxtest_keyboard_interrupt(excinfo):
    """React when the user interrupts the test session with Ctrl+C."""

    _ = excinfo


def test_hook_examples_module_imports():
    """Basic smoke test so this file participates in the example suite."""

    assert Path(__file__).name == "test_hooks.py"
