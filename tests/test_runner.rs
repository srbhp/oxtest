use oxtest::{discover_tests, list_fixtures, list_fixtures_per_test, run_tests, CaptureMode, RunConfig};
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
fn run_tests_executes_oxtest_fixture_dependent_test() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_oxtest_fixture.py");
    fs::write(
        &path,
        r#"
import oxtest

@oxtest.fixture
def my_fixture():
    return 42

@oxtest.fixture
def my_other_fixture(my_fixture):
    return my_fixture * 2


def test_with_oxtest_fixtures(my_fixture, my_other_fixture):
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
    assert_eq!(summary.results[0].full_name, "test_with_oxtest_fixtures");
}

#[test]
fn run_tests_supports_oxtest_mark_parametrize() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_oxtest_mark.py");
    fs::write(
        &path,
        r#"
import oxtest

@oxtest.mark.parametrize(
    "value, expected",
    [
        oxtest.param(1, 1, id="one"),
        oxtest.param(2, 2, id="two"),
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
