use anyhow::{anyhow, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use super::plugin::plugin_registry;
use super::python::PY_HELPER;
use super::types::{FixtureUsage, RunConfig, TestItem};

pub fn discover_tests(path: &str, config: &RunConfig) -> Result<Vec<TestItem>> {
    let mut config = config.clone();
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.configure(&mut config);
    }

    Python::with_gil(|py| -> Result<()> {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let reset = module.getattr("_ensure_oxtest_module")?;
        reset.call0()?;
        let oxtest = PyModule::import(py, "oxtest")?;
        oxtest.getattr("_reset_hooks")?.call0()?;
        Ok(())
    })?;

    let base = Path::new(path);
    let globset = build_ignore_globset(&config.ignore_glob)?;
    let walker = WalkDir::new(base).into_iter();
    let mut tests = Vec::new();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        let ext = entry.path().extension().and_then(OsStr::to_str);
        if ext != Some("py") {
            continue;
        }

        if is_ignored(entry.path(), base, &config.ignore, &globset) {
            continue;
        }
        if hook_ignores_path(entry.path(), &config)? {
            continue;
        }

        if let Some(items) = discover_file(entry.path())? {
            for item in items {
                if let Some(k_expr) = config.k_expr.as_deref() {
                    if !match_keyword(k_expr, &item.full_name, &item.extra_keywords)? {
                        continue;
                    }
                }
                if let Some(m_expr) = config.m_expr.as_deref() {
                    if !match_markexpr(m_expr, &item.marks)? {
                        continue;
                    }
                }
                tests.push(item);
            }
        }
    }

    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.collect(&mut tests);
    }

    tests.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(tests)
}

pub fn list_markers(path: &str, config: &RunConfig) -> Result<Vec<String>> {
    let tests = discover_tests(path, config)?;
    let mut marker_set = std::collections::BTreeSet::new();
    for item in tests {
        for mark in item.marks {
            marker_set.insert(mark);
        }
    }
    Ok(marker_set.into_iter().collect())
}

pub fn list_fixtures(path: &str, config: &RunConfig) -> Result<Vec<String>> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let list_fn = module.getattr("list_fixtures")?;
        let result = list_fn.call1((path,))?;
        let fixtures: Vec<String> = result.extract()?;
        let mut fixtures: Vec<String> = fixtures
            .into_iter()
            .filter(|fixture| config.verbose > 0 || !fixture.starts_with('_'))
            .collect();
        fixtures.sort();
        Ok(fixtures)
    })
}

pub fn list_fixtures_per_test(path: &str, config: &RunConfig) -> Result<Vec<FixtureUsage>> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let list_fn = module.getattr("fixtures_per_test")?;
        let result = list_fn.call1((path,))?;
        let list = result
            .downcast::<PyList>()
            .map_err(|err| anyhow!("Python fixtures_per_test returned unexpected value: {}", err))?;
        let mut usage = Vec::new();

        for item in list.iter() {
            let dict = item
                .downcast::<PyDict>()
                .map_err(|err| anyhow!("Python fixtures_per_test entry was not a dict: {}", err))?;
            let test: String = dict.get_item("name").unwrap().extract()?;
            let fixtures: Vec<String> = dict.get_item("fixtures").unwrap().extract()?;
            let fixtures: Vec<String> = fixtures
                .into_iter()
                .filter(|fixture| config.verbose > 0 || !fixture.starts_with('_'))
                .collect();
            usage.push(FixtureUsage { test, fixtures });
        }

        Ok(usage)
    })
}

fn build_ignore_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|err| anyhow!("Invalid ignore-glob pattern {}: {}", pattern, err))?);
    }
    builder
        .build()
        .map_err(|err| anyhow!("Invalid ignore-glob set: {}", err))
}

fn is_ignored(path: &Path, base: &Path, ignore_paths: &[PathBuf], globset: &GlobSet) -> bool {
    if ignore_paths.iter().any(|ignore| matches_ignore_path(path, base, ignore)) {
        return true;
    }
    globset.is_match(path)
}

fn matches_ignore_path(path: &Path, base: &Path, ignore: &Path) -> bool {
    if ignore.is_absolute() {
        path.starts_with(ignore)
    } else if let Ok(relative) = path.strip_prefix(base) {
        relative.starts_with(ignore)
    } else {
        false
    }
}

fn discover_file(path: &Path) -> Result<Option<Vec<TestItem>>> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let discover = module.getattr("discover_with_hooks")?;
        let result = discover.call1((path.to_str().unwrap(),))?;
        let list = result
            .downcast::<PyList>()
            .map_err(|err| anyhow!("Python discovery returned unexpected value: {}", err))?;
        let mut items = Vec::new();

        for item in list.iter() {
            let dict = item
                .downcast::<PyDict>()
                .map_err(|err| anyhow!("Python discovery item was not a dict: {}", err))?;
            let name: String = dict.get_item("name").unwrap().extract()?;
            let marks: Vec<String> = dict.get_item("marks").unwrap().extract()?;
            let extra_keywords: Vec<String> = dict.get_item("extra_keywords").unwrap().extract()?;
            items.push(TestItem {
                file: path.to_path_buf(),
                name: name.clone(),
                full_name: name,
                marks,
                extra_keywords,
            });
        }

        Ok(Some(items))
    })
}

fn hook_ignores_path(path: &Path, config: &RunConfig) -> Result<bool> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let ignore = module.getattr("should_ignore_collect")?;
        let config_dict = PyDict::new(py);
        config_dict.set_item("k_expr", config.k_expr.clone())?;
        config_dict.set_item("m_expr", config.m_expr.clone())?;
        config_dict.set_item("exitfirst", config.exitfirst)?;
        config_dict.set_item("maxfail", config.maxfail)?;
        config_dict.set_item("jobs", config.jobs)?;
        config_dict.set_item("ignore_glob", config.ignore_glob.clone())?;
        config_dict.set_item("collect_only", config.collect_only)?;
        config_dict.set_item("quiet", config.quiet)?;
        config_dict.set_item("verbose", config.verbose)?;
        config_dict.set_item("strict", config.strict)?;
        config_dict.set_item("strict_markers", config.strict_markers)?;
        config_dict.set_item("strict_config", config.strict_config)?;
        let result = ignore.call1((path.to_str().unwrap(), config_dict))?;
        result.extract()
    })
    .map_err(|err| anyhow!("Failed to run ignore_collect hook: {}", err))
}

fn match_keyword(expr: &str, test_name: &str, extra_names: &[String]) -> Result<bool> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let matcher = module.getattr("match_keyword")?;
        let extra = extra_names.to_object(py);
        let result = matcher.call1((expr, test_name, extra))?;
        result.extract()
    })
    .map_err(|err| anyhow!("Failed to run keyword matcher: {}", err))
}

fn match_markexpr(expr: &str, marks: &[String]) -> Result<bool> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let matcher = module.getattr("match_markexpr")?;
        let py_marks = marks.to_object(py);
        let result = matcher.call1((expr, py_marks))?;
        result.extract()
    })
    .map_err(|err| anyhow!("Failed to run mark expression matcher: {}", err))
}
