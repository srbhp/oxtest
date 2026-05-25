use cobratest::{discover_tests, list_fixtures, list_fixtures_per_test, run_tests, CaptureMode, RunConfig};
use std::fs;
use tempfile::tempdir;

fn make_config() -> RunConfig {
    RunConfig {
        k_expr: None,
        m_expr: None,
        exitfirst: false,
        maxfail: None,
        jobs: 1,
        ignore: Vec::new(),
        ignore_glob: Vec::new(),
        capture: CaptureMode::No,
        collect_only: false,
        quiet: false,
        verbose: 0,
        strict: false,
        strict_markers: false,
        strict_config: false,
    }
}

#[test]
fn discover_tests_finds_functions_and_test_classes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_example.py");
    fs::write(
        &path,
        r#"
import pytest

@pytest.mark.smoke
def test_one():
    pass

class TestExample:
    def test_two(self):
        pass
"#,
    )
    .unwrap();

    let config = make_config();
    let items = discover_tests(dir.path().to_str().unwrap(), &config).unwrap();
    let names: Vec<String> = items.iter().map(|item| item.full_name.clone()).collect();

    assert_eq!(names, vec!["TestExample.test_two".to_string(), "test_one".to_string()]);
    assert!(items.iter().any(|item| item.full_name == "test_one" && item.marks.contains(&"smoke".to_string())));
}

#[test]
fn discover_parametrized_test_names_and_filter_by_keyword() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_params.py");
    fs::write(
        &path,
        r#"
import pytest

@pytest.mark.parametrize("x", [1, 2])
def test_values(x):
    assert x in (1, 2)

@pytest.mark.smoke
def test_smoke():
    assert True
"#,
    )
    .unwrap();

    let config = make_config();
    let items = discover_tests(dir.path().to_str().unwrap(), &config).unwrap();
    let names: Vec<String> = items.iter().map(|item| item.full_name.clone()).collect();

    assert!(names.contains(&"test_values[0]".to_string()));
    assert!(names.contains(&"test_values[1]".to_string()));
    assert!(names.contains(&"test_smoke".to_string()));

    let filtered: Vec<String> = items
        .into_iter()
        .filter(|item| item.full_name.contains("values"))
        .map(|item| item.full_name)
        .collect();

    assert_eq!(filtered, vec!["test_values[0]".to_string(), "test_values[1]".to_string()]);
}

#[test]
fn run_tests_executes_simple_test_with_keyword_filter() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_simple.py");
    fs::write(
        &path,
        r#"
import pytest

def test_values():
    assert True

@pytest.mark.smoke
def test_smoke():
    assert True
"#,
    )
    .unwrap();

    let mut config = make_config();
    config.k_expr = Some("values".into());
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.results.len(), 1);
    assert_eq!(summary.results[0].full_name, "test_values");
}

#[test]
fn list_fixtures_reports_fixture_usage_for_test_functions() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_fixtures.py");
    fs::write(
        &path,
        r#"
import pytest

@pytest.fixture
def my_fixture():
    return 42


def test_with_fixture(my_fixture):
    assert my_fixture == 42
"#,
    )
    .unwrap();

    let config = make_config();
    let fixtures = list_fixtures(path.to_str().unwrap(), &config).unwrap();
    assert_eq!(fixtures, vec!["my_fixture".to_string()]);

    let usage = list_fixtures_per_test(path.to_str().unwrap(), &config).unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].test, "test_with_fixture".to_string());
    assert_eq!(usage[0].fixtures, vec!["my_fixture".to_string()]);
}

#[test]
fn run_tests_executes_cobratest_fixture_dependent_test() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_cobratest_fixture.py");
    fs::write(
        &path,
        r#"
import cobratest

@cobratest.fixture
def my_fixture():
    return 42

@cobratest.fixture
def my_other_fixture(my_fixture):
    return my_fixture * 2


def test_with_cobratest_fixtures(my_fixture, my_other_fixture):
    assert my_fixture == 42
    assert my_other_fixture == 84
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.results.len(), 1);
    assert_eq!(summary.results[0].full_name, "test_with_cobratest_fixtures");
}

#[test]
fn run_tests_supports_cobratest_mark_parametrize() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_cobratest_mark.py");
    fs::write(
        &path,
        r#"
import cobratest

@cobratest.mark.parametrize(
    "value, expected",
    [
        cobratest.param(1, 1, id="one"),
        cobratest.param(2, 2, id="two"),
    ],
)
def test_param_example(value, expected):
    assert value == expected
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 2);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.results.len(), 2);
    assert!(summary
        .results
        .iter()
        .any(|item| item.full_name.starts_with("test_param_example")));
}

#[test]
fn run_tests_supports_cobratest_mark_behavior() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_cobratest_mark_behavior.py");
    fs::write(
        &path,
        r#"
import cobratest

@cobratest.fixture
def my_fixture():
    return 42

@cobratest.mark.usefixtures("my_fixture")
def test_usefixtures():
    assert True

@cobratest.mark.skip(reason="skip example")
def test_skip():
    assert False

@cobratest.mark.skipif(True, reason="skipif example")
def test_skipif():
    assert False

@cobratest.mark.xfail(reason="expected failure")
def test_xfail():
    raise AssertionError("boom")

@cobratest.mark.custom
def test_custom_mark():
    assert True
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 5);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.results.len(), 5);
    assert!(summary.results.iter().any(|item| item.full_name == "test_usefixtures"));
    assert!(summary.results.iter().any(|item| item.full_name == "test_skip"));
    assert!(summary.results.iter().any(|item| item.full_name == "test_skipif"));
    assert!(summary.results.iter().any(|item| item.full_name == "test_xfail"));
    assert!(summary.results.iter().any(|item| item.full_name == "test_custom_mark"));
}

#[test]
fn run_tests_supports_pytest_shim_helpers() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_pytest_shim.py");
    fs::write(
        &path,
        r#"
import warnings
import pytest


def test_approx():
    assert 0.1 + 0.2 == pytest.approx(0.3)


def test_raises():
    with pytest.raises(ValueError):
        raise ValueError("boom")


def test_warns():
    with pytest.warns(DeprecationWarning):
        warnings.warn("deprecated", DeprecationWarning)


def test_importorskip():
    assert pytest.importorskip("sys") is not None


def test_main():
    assert pytest.main() == 0


def test_param():
    p = pytest.param(1, id="one")
    assert p[0] == 1


def test_xfail():
    pytest.xfail("expected failure")
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 7);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.results.len(), 7);
    let xfail_result = summary
        .results
        .iter()
        .find(|item| item.full_name == "test_xfail")
        .unwrap();
    assert!(xfail_result.output.contains("xfail"));
}

#[test]
fn discover_tests_respects_cobratest_ignore_collect_hook() {
    let dir = tempdir().unwrap();
    let conftest = dir.path().join("conftest.py");
    let kept = dir.path().join("test_kept.py");
    let ignored = dir.path().join("test_ignored.py");

    fs::write(
        &conftest,
        r#"
import cobratest

@cobratest.hookimpl
def cobratest_ignore_collect(collection_path, config):
    return collection_path.name == "test_ignored.py"
"#,
    )
    .unwrap();

    fs::write(&kept, "def test_kept():\n    assert True\n").unwrap();
    fs::write(&ignored, "def test_ignored():\n    assert True\n").unwrap();

    let config = make_config();
    let items = discover_tests(dir.path().to_str().unwrap(), &config).unwrap();
    let names: Vec<String> = items.into_iter().map(|item| item.full_name).collect();

    assert_eq!(names, vec!["test_kept".to_string()]);
}

#[test]
fn run_tests_executes_cobratest_hook_lifecycle() {
    let dir = tempdir().unwrap();
    let conftest = dir.path().join("conftest.py");
    let path = dir.path().join("test_hooks_runtime.py");
    let log_path = dir.path().join("hook_log.txt");

    fs::write(
        &conftest,
        format!(
            r#"
import pathlib
import cobratest

LOG = pathlib.Path(r"{log}")

def _log(message):
    with LOG.open("a", encoding="utf-8") as handle:
        handle.write(message + "\n")

class ExampleSpecs:
    @cobratest.hookspec(firstresult=True)
    def cobratest_example_transform(self, value):
        """Return a transformed value."""

@cobratest.hookimpl
def cobratest_addhooks(pluginmanager):
    pluginmanager.add_hookspecs(ExampleSpecs)
    _log("addhooks")

@cobratest.hookimpl
def cobratest_plugin_registered(plugin, plugin_name, manager):
    _log(f"registered:{{plugin_name}}")

@cobratest.hookimpl
def cobratest_addoption(parser):
    parser.addoption("--demo-flag", action="store_true", default=True, help="demo")
    _log("addoption")

@cobratest.hookimpl
def cobratest_configure(config):
    _log(f"configure:{{config.getoption('--demo-flag')}}")

@cobratest.hookimpl
def cobratest_report_header(config):
    _log("report_header")
    return ["hook header"]

@cobratest.hookimpl
def cobratest_sessionstart(session):
    _log("sessionstart")

@cobratest.hookimpl
def cobratest_collection_modifyitems(config, items):
    _log(f"modifyitems:{{len(items)}}")
    for item in items:
        item.add_marker("hooked")

@cobratest.hookimpl
def cobratest_runtest_setup(item):
    _log(f"setup:{{item.full_name}}")

@cobratest.hookimpl
def cobratest_runtest_call(item):
    _log(f"call:{{item.full_name}}")

@cobratest.hookimpl
def cobratest_runtest_teardown(item, nextitem):
    next_name = getattr(nextitem, "full_name", None)
    _log(f"teardown:{{item.full_name}}:{{next_name}}")

@cobratest.hookimpl(wrapper=True)
def cobratest_runtest_makereport(item, call):
    _log(f"makereport-before:{{item.full_name}}")
    outcome = yield
    report = outcome.get_result()
    _log(f"makereport-after:{{item.full_name}}:{{report.outcome}}")

@cobratest.hookimpl
def cobratest_exception_interact(node, call, report):
    _log(f"exception:{{node.full_name}}:{{report.outcome}}")

@cobratest.hookimpl
def cobratest_sessionfinish(session, exitstatus):
    _log(f"sessionfinish:{{exitstatus}}")

@cobratest.hookimpl
def cobratest_terminal_summary(terminalreporter, exitstatus, config):
    _log(f"terminal_summary:{{exitstatus}}")
"#,
            log = log_path.display()
        ),
    )
    .unwrap();

    fs::write(
        &path,
        r#"
def test_pass():
    assert True

def test_fail():
    assert False
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();
    let log = fs::read_to_string(&log_path).unwrap();

    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 1);
    assert!(summary
        .results
        .iter()
        .all(|result| result.marks.iter().any(|mark| mark == "hooked")));
    assert!(log.contains("addhooks"));
    assert!(log.contains("addoption"));
    assert!(log.contains("configure:True"));
    assert!(log.contains("report_header"));
    assert!(log.contains("sessionstart"));
    assert!(log.contains("modifyitems:2"));
    assert!(log.contains("setup:test_fail"));
    assert!(log.contains("call:test_fail"));
    assert!(log.contains("exception:test_fail:failed"));
    assert!(log.contains("makereport-before:test_fail"));
    assert!(log.contains("makereport-after:test_fail:failed"));
    assert!(log.contains("teardown:test_pass:None") || log.contains("teardown:test_pass:test_fail"));
    assert!(log.contains("sessionfinish:1"));
    assert!(log.contains("terminal_summary:1"));
}

#[test]
fn run_tests_exposes_collection_tree_objects() {
    let dir = tempdir().unwrap();
    let pkg_dir = dir.path().join("pkg");
    let conftest = dir.path().join("conftest.py");
    let init_py = pkg_dir.join("__init__.py");
    let path = pkg_dir.join("test_tree.py");
    let log_path = dir.path().join("collection_tree_log.txt");

    fs::create_dir_all(&pkg_dir).unwrap();
    fs::write(&init_py, "").unwrap();

    fs::write(
        &conftest,
        format!(
            r#"
import pathlib
import cobratest

LOG = pathlib.Path(r"{log}")

def _log(message):
    with LOG.open("a", encoding="utf-8") as handle:
        handle.write(message + "\n")

assert issubclass(cobratest.Collector, cobratest.Node)
assert issubclass(cobratest.Item, cobratest.Node)
assert issubclass(cobratest.File, cobratest.FSCollector)
assert issubclass(cobratest.FSCollector, cobratest.Collector)
assert issubclass(cobratest.Session, cobratest.Collector)
assert issubclass(cobratest.Package, cobratest.FSCollector)
assert issubclass(cobratest.Module, cobratest.File)
assert issubclass(cobratest.Class, cobratest.Collector)
assert issubclass(cobratest.Function, cobratest.Item)
assert issubclass(cobratest.FunctionDefinition, cobratest.Collector)

@cobratest.hookimpl
def cobratest_sessionstart(session):
    _log(
        "session:"
        + str(isinstance(session, cobratest.Session))
        + ":"
        + str(isinstance(session, cobratest.Collector))
        + ":"
        + str(isinstance(session, cobratest.Node))
    )

@cobratest.hookimpl
def cobratest_collect_file(file_path, parent):
    if file_path.name == "test_tree.py":
        _log(
            "collect_file:"
            + type(parent).__name__
            + ":"
            + str(isinstance(parent, cobratest.Module))
            + ":"
            + str(isinstance(parent, cobratest.File))
        )
    return None

@cobratest.hookimpl
def cobratest_collection_modifyitems(config, items):
    for item in items:
        if item.full_name == "TestExample.test_child":
            chain = ">".join(type(node).__name__ for node in item.listchain())
            _log("item_type:" + type(item).__name__)
            _log("function_parent:" + type(item.parent).__name__)
            _log("class_parent:" + type(item.getparent(cobratest.Class)).__name__)
            _log("module_parent:" + type(item.getparent(cobratest.Module)).__name__)
            package = item.getparent(cobratest.Package)
            _log("package_parent:" + getattr(package, "name", ""))
            _log("chain:" + chain)
"#,
            log = log_path.display()
        ),
    )
    .unwrap();

    fs::write(
        &path,
        r#"
import cobratest

def test_top_level():
    assert issubclass(cobratest.FunctionDefinition, cobratest.Collector)

class TestExample:
    def test_child(self):
        assert issubclass(cobratest.Module, cobratest.File)
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();
    let log = fs::read_to_string(&log_path).unwrap();

    assert_eq!(summary.passed, 2);
    assert_eq!(summary.failed, 0);
    assert!(log.contains("session:True:True:True"));
    assert!(log.contains("collect_file:_Module:True:True"));
    assert!(log.contains("item_type:_Function"));
    assert!(log.contains("function_parent:_FunctionDefinition"));
    assert!(log.contains("class_parent:_Class"));
    assert!(log.contains("module_parent:_Module"));
    assert!(log.contains("package_parent:pkg"));
    assert!(log.contains("chain:_Session>_Package>_Module>_Class>_FunctionDefinition>_Function"));
}

#[test]
fn run_tests_reports_pytest_fail() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_pytest_fail.py");
    fs::write(
        &path,
        r#"
import pytest

def test_mark_fail():
    pytest.fail("boom")
"#,
    )
    .unwrap();

    let config = make_config();
    let summary = run_tests(dir.path().to_str().unwrap(), config).unwrap();

    assert_eq!(summary.passed, 0);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.results.len(), 1);
    assert!(summary.results[0].output.contains("boom"));
}
