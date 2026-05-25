"""Examples of cobratest hook APIs and plugin extension points.

These are lightweight examples meant to document hook usage patterns.
Most hooks are only meaningful when exposed from a plugin module or a
``conftest.py`` file, so this module focuses on importable examples
instead of trying to execute every hook in a live cobratest session.
"""

from __future__ import annotations

from pathlib import Path

import cobratest

hookimpl = getattr(cobratest, "hookimpl", lambda *args, **kwargs: lambda func: func)
hookspec = getattr(cobratest, "hookspec", lambda *args, **kwargs: lambda func: func)


# ---------------------------------------------------------------------------
# @cobratest.hookimpl / @cobratest.hookspec
# ---------------------------------------------------------------------------


class ExampleSpecs:
    """Custom hook specifications for a toy plugin API."""

    @hookspec(firstresult=True)
    def cobratest_example_transform(self, value):
        """Return a transformed value from the first plugin that can handle it."""


class ExamplePlugin:
    """Custom hook implementations matching ExampleSpecs."""

    @hookimpl
    def cobratest_example_transform(self, value):
        if isinstance(value, str):
            return value.upper()
        return None


# ---------------------------------------------------------------------------
# Bootstrapping hooks
# ---------------------------------------------------------------------------


@hookimpl
def cobratest_addhooks(pluginmanager):
    """Register additional hookspecs during cobratest startup."""

    pluginmanager.add_hookspecs(ExampleSpecs)


@hookimpl
def cobratest_plugin_registered(plugin, plugin_name, manager):
    """Observe plugins as they are registered."""

    _ = (plugin, plugin_name, manager)


# ---------------------------------------------------------------------------
# Initialization hooks
# ---------------------------------------------------------------------------


@hookimpl
def cobratest_addoption(parser):
    """Add CLI flags or ini options before collection starts."""

    parser.addoption(
        "--demo-flag",
        action="store_true",
        default=False,
        help="Enable the demo hook examples.",
    )


@hookimpl
def cobratest_configure(config):
    """Perform one-time plugin setup after options are parsed."""

    config.addinivalue_line("markers", "demo: mark tests that belong to hook demos")


@hookimpl
def cobratest_sessionstart(session):
    """Run setup code when the test session begins."""

    _ = session


@hookimpl
def cobratest_sessionfinish(session, exitstatus):
    """Run teardown code at the end of the test session."""

    _ = (session, exitstatus)


# ---------------------------------------------------------------------------
# Collection hooks
# ---------------------------------------------------------------------------


assert issubclass(cobratest.Collector, cobratest.Node)
assert issubclass(cobratest.Item, cobratest.Node)
assert issubclass(cobratest.File, cobratest.FSCollector)
assert issubclass(cobratest.Session, cobratest.Collector)
assert issubclass(cobratest.Package, cobratest.FSCollector)
assert issubclass(cobratest.Module, cobratest.File)
assert issubclass(cobratest.Class, cobratest.Collector)
assert issubclass(cobratest.Function, cobratest.Item)
assert issubclass(cobratest.FunctionDefinition, cobratest.Collector)


@hookimpl
def cobratest_ignore_collect(collection_path, config):
    """Skip generated folders during collection."""

    _ = config
    return "generated" in str(collection_path)


@hookimpl
def cobratest_collect_file(file_path, parent):
    """Inspect files and decide whether to create a custom collector."""

    _ = parent
    if file_path.suffix == ".hookdemo":
        return None
    return None


@hookimpl
def cobratest_collection_modifyitems(config, items):
    """Reorder or mark collected tests before execution."""

    _ = config
    for item in items:
        if "hooks" in item.name:
            item.add_marker(cobratest.mark.demo)
        _ = item.nodeid
        _ = item.listchain()
        _ = item.getparent(cobratest.Module)


# ---------------------------------------------------------------------------
# Test running (runtest) hooks
# ---------------------------------------------------------------------------


@hookimpl
def cobratest_runtest_setup(item):
    """Run before each test's setup phase."""

    _ = item


@hookimpl
def cobratest_runtest_call(item):
    """Run immediately before the test function body."""

    _ = item


@hookimpl
def cobratest_runtest_teardown(item, nextitem):
    """Run after each test's teardown phase."""

    _ = (item, nextitem)


@hookimpl(wrapper=True)
def cobratest_runtest_makereport(item, call):
    """Wrap report creation to inspect pass/fail information."""

    outcome = yield
    report = outcome.get_result()
    if report.when == "call" and report.failed:
        item.user_properties.append(("failed_in_demo_hook", True))


# ---------------------------------------------------------------------------
# Reporting hooks
# ---------------------------------------------------------------------------


@hookimpl
def cobratest_report_header(config):
    """Add custom lines near the top of the cobratest report."""

    if config.getoption("--demo-flag"):
        return ["demo-flag enabled for hook examples"]
    return []


@hookimpl
def cobratest_terminal_summary(terminalreporter, exitstatus, config):
    """Emit summary information after the test run completes."""

    _ = (exitstatus, config)
    terminalreporter.write_line("hook demo summary complete")


# ---------------------------------------------------------------------------
# Debugging / interaction hooks
# ---------------------------------------------------------------------------


@hookimpl
def cobratest_exception_interact(node, call, report):
    """Inspect state when an interactive exception is raised."""

    _ = (node, call, report)


@hookimpl
def cobratest_enter_pdb(config, pdb):
    """Run immediately before cobratest drops into pdb."""

    _ = (config, pdb)


@hookimpl
def cobratest_keyboard_interrupt(excinfo):
    """React when the user interrupts the test session with Ctrl+C."""

    _ = excinfo


def test_hook_examples_module_imports():
    """Basic smoke test so this file participates in the example suite."""

    assert Path(__file__).name == "test_hooks.py"
