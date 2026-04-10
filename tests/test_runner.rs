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
