use anyhow::{anyhow, Result};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use super::plugin::plugin_registry;
use super::discovery::discover_tests;
use super::python::PY_HELPER;
use super::types::{RunConfig, TestItem, TestResult, TestSummary};

#[derive(Deserialize)]
struct MarshaledTestResult {
    passed: bool,
    output: String,
}

fn test_item_to_py<'py>(py: Python<'py>, test: &TestItem) -> PyObject {
    let dict = PyDict::new(py);
    dict.set_item("file", test.file.to_string_lossy().to_string()).unwrap();
    dict.set_item("name", test.name.clone()).unwrap();
    dict.set_item("full_name", test.full_name.clone()).unwrap();
    dict.set_item("marks", test.marks.clone()).unwrap();
    dict.set_item("extra_keywords", test.extra_keywords.clone()).unwrap();
    dict.into()
}

fn test_result_to_py<'py>(py: Python<'py>, result: &TestResult) -> PyObject {
    let dict = PyDict::new(py);
    dict.set_item("file", result.file.to_string_lossy().to_string()).unwrap();
    dict.set_item("full_name", result.full_name.clone()).unwrap();
    dict.set_item("passed", result.passed).unwrap();
    dict.set_item("output", result.output.clone()).unwrap();
    dict.set_item("marks", result.marks.clone()).unwrap();
    dict.into()
}

fn config_to_py<'py>(py: Python<'py>, config: &RunConfig) -> PyObject {
    let dict = PyDict::new(py);
    dict.set_item("k_expr", config.k_expr.clone()).unwrap();
    dict.set_item("m_expr", config.m_expr.clone()).unwrap();
    dict.set_item("exitfirst", config.exitfirst).unwrap();
    dict.set_item("maxfail", config.maxfail).unwrap();
    dict.set_item("jobs", config.jobs).unwrap();
    dict.set_item(
        "ignore",
        config
            .ignore
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    dict.set_item("ignore_glob", config.ignore_glob.clone()).unwrap();
    dict.set_item("capture", format!("{:?}", config.capture)).unwrap();
    dict.set_item("collect_only", config.collect_only).unwrap();
    dict.set_item("quiet", config.quiet).unwrap();
    dict.set_item("verbose", config.verbose).unwrap();
    dict.set_item("strict", config.strict).unwrap();
    dict.set_item("strict_markers", config.strict_markers).unwrap();
    dict.set_item("strict_config", config.strict_config).unwrap();
    dict.into()
}

fn summary_to_py<'py>(py: Python<'py>, summary: &TestSummary) -> PyObject {
    let dict = PyDict::new(py);
    let results = PyList::empty(py);
    for result in &summary.results {
        results.append(test_result_to_py(py, result)).unwrap();
    }
    dict.set_item("results", results).unwrap();
    dict.set_item("passed", summary.passed).unwrap();
    dict.set_item("failed", summary.failed).unwrap();
    dict.into()
}

fn import_test_modules(tests: &[TestItem]) -> Result<()> {
    let mut imported_paths = HashSet::new();
    Python::with_gil(|py| {
        let helper = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let load_module = helper.getattr("load_module")?;
        for test in tests {
            let path = test.file.to_string_lossy().to_string();
            if imported_paths.insert(path.clone()) {
                load_module.call1((path,))?;
            }
        }
        Ok(())
    })
}

fn apply_collection_hooks(tests: Vec<TestItem>) -> Result<Vec<TestItem>> {
    Python::with_gil(|py| {
        let helper = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let py_tests = PyList::empty(py);
        for test in &tests {
            py_tests.append(test_item_to_py(py, test))?;
        }
        let apply = helper.getattr("apply_collection_hooks")?;
        let result = apply.call1((py_tests,))?;
        let result_list = result
            .downcast::<PyList>()
            .map_err(|err| anyhow!("Collection hook returned invalid result list: {}", err))?;
        let mut modified_tests = Vec::new();
        for item in result_list.iter() {
            let dict = item
                .downcast::<PyDict>()
                .map_err(|err| anyhow!("Collection hook returned invalid test item: {}", err))?;
            let file: String = dict.get_item("file").unwrap().extract()?;
            let name: String = dict.get_item("name").unwrap().extract()?;
            let full_name: String = dict.get_item("full_name").unwrap().extract()?;
            let marks: Vec<String> = dict.get_item("marks").unwrap().extract()?;
            let extra_keywords: Vec<String> = dict.get_item("extra_keywords").unwrap().extract()?;
            modified_tests.push(TestItem {
                file: PathBuf::from(file),
                name,
                full_name,
                marks,
                extra_keywords,
            });
        }
        Ok(modified_tests)
    })
}

pub fn run_tests(path: &str, config: RunConfig) -> Result<TestSummary> {
    let mut config = config;
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.configure(&mut config);
    }

    let tests = discover_tests(path, &config)?;
    import_test_modules(&tests)?;

    Python::with_gil(|py| -> Result<()> {
        let helper = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let begin = helper.getattr("begin_test_session")?;
        begin.call1((path, config_to_py(py, &config)))?;
        Ok(())
    })?;

    let tests = apply_collection_hooks(tests)?;
    if tests.is_empty() {
        return Ok(TestSummary {
            results: Vec::new(),
            passed: 0,
            failed: 0,
        });
    }

    let results = if config.exitfirst || config.maxfail.is_some() || config.jobs <= 1 {
        run_tests_sequential(tests, &config)
    } else {
        run_tests_parallel(tests, &config)
    }?;

    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;
    let summary = TestSummary { results, passed, failed };

    Python::with_gil(|py| -> Result<()> {
        let helper = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let finish = helper.getattr("finish_test_session")?;
        finish.call1((summary_to_py(py, &summary), config_to_py(py, &config)))?;
        Ok(())
    })?;

    Ok(summary)
}

fn run_tests_sequential(tests: Vec<TestItem>, config: &RunConfig) -> Result<Vec<TestResult>> {
    let mut results = Vec::new();
    let mut failures = 0;

    for (index, test) in tests.iter().enumerate() {
        let next_test = tests.get(index + 1);
        let result = run_test_item(test, next_test, config)?;
        for plugin in plugin_registry().lock().unwrap().iter() {
            plugin.after_test(test, &result);
        }
        if !result.passed {
            failures += 1;
        }
        results.push(result);

        if config.exitfirst && failures > 0 {
            break;
        }
        if let Some(maxfail) = config.maxfail {
            if failures >= maxfail {
                break;
            }
        }
    }

    Ok(results)
}

fn run_tests_parallel(tests: Vec<TestItem>, _config: &RunConfig) -> Result<Vec<TestResult>> {
    let plugins = plugin_registry().lock().unwrap().clone();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(_config.jobs).build()?;
    pool.install(|| {
        tests
            .into_par_iter()
            .map(|test| {
                let result = run_test_item(&test, None, _config)?;
                for plugin in &plugins {
                    plugin.after_test(&test, &result);
                }
                Ok(result)
            })
            .collect::<Result<Vec<_>, _>>()
    })
}

pub fn run_test_item(test: &TestItem, next_test: Option<&TestItem>, config: &RunConfig) -> Result<TestResult> {
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.before_test(test);
    }
    let (passed, output) = run_test(test, next_test, config)?;
    let result = TestResult {
        file: test.file.clone(),
        full_name: test.full_name.clone(),
        passed,
        output,
        marks: test.marks.clone(),
    };
    Ok(result)
}

fn run_test(test: &TestItem, next_test: Option<&TestItem>, config: &RunConfig) -> Result<(bool, String)> {
    Python::with_gil(|py| {
        let helper = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let run_test = helper.getattr("run_test_marshaled")?;
        let next_name = next_test.map(|item| item.full_name.clone());
        let result = run_test.call1((
            test.file.to_str().unwrap(),
            test.full_name.as_str(),
            config_to_py(py, config),
            next_name,
        ))?;
        let payload: String = result.extract()?;
        let parsed: MarshaledTestResult = serde_json::from_str(&payload)
            .map_err(|err| anyhow!("Failed to parse Python test result payload: {}", err))?;
        Ok((parsed.passed, parsed.output))
    })
}
