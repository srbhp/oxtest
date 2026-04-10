use anyhow::{anyhow, Result};
use clap::ValueEnum;
use globset::{Glob, GlobSet, GlobSetBuilder};
use pyo3::prelude::*;
use rayon::prelude::*;
use serde::Serialize;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use walkdir::WalkDir;

static PLUGIN_REGISTRY: OnceLock<Mutex<Vec<Arc<dyn OxtestPlugin>>>> = OnceLock::new();

fn plugin_registry() -> &'static Mutex<Vec<Arc<dyn OxtestPlugin>>> {
    PLUGIN_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

pub trait OxtestPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn configure(&self, _config: &mut RunConfig) {}
    fn collect(&self, _tests: &mut Vec<TestItem>) {}
    fn before_test(&self, _test: &TestItem) {}
    fn after_test(&self, _test: &TestItem, _result: &TestResult) {}
}

pub fn register_plugin(plugin: Arc<dyn OxtestPlugin>) {
    plugin_registry().lock().unwrap().push(plugin);
}

#[derive(Clone, Debug, Serialize, ValueEnum)]
pub enum CaptureMode {
    Fd,
    Sys,
    No,
    TeeSys,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunConfig {
    pub k_expr: Option<String>,
    pub m_expr: Option<String>,
    pub exitfirst: bool,
    pub maxfail: Option<usize>,
    pub jobs: usize,
    pub ignore: Vec<PathBuf>,
    pub ignore_glob: Vec<String>,
    pub capture: CaptureMode,
    pub collect_only: bool,
    pub quiet: bool,
    pub verbose: u8,
    pub strict: bool,
    pub strict_markers: bool,
    pub strict_config: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TestItem {
    pub file: PathBuf,
    pub name: String,
    pub full_name: String,
    pub marks: Vec<String>,
    pub extra_keywords: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TestResult {
    pub file: PathBuf,
    pub full_name: String,
    pub passed: bool,
    pub output: String,
    pub marks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TestSummary {
    pub results: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct FixtureUsage {
    pub test: String,
    pub fixtures: Vec<String>,
}

impl TestSummary {
    pub fn print_summary(&self) {
        for result in &self.results {
            if result.passed {
                println!("ok {}:{}", result.file.display(), result.full_name);
            } else {
                println!("FAILED {}:{}", result.file.display(), result.full_name);
                println!("{}\n", result.output.trim_end());
            }
        }

        println!("\nSummary: {} passed, {} failed", self.passed, self.failed);
    }
}

const PY_HELPER: &str = r#"
import ast
import importlib.util
import pathlib
import sys
import traceback
import re
import inspect
import warnings


class _SkipTest(Exception):
    pass


class _XFailed(Exception):
    pass


def _normalize_parametrize(decorator):
    if not isinstance(decorator, ast.Call):
        return None
    func = decorator.func
    if isinstance(func, ast.Attribute) and func.attr == "parametrize":
        args = decorator.args
        if len(args) < 2:
            return None
        values = args[1]
        if isinstance(values, ast.List):
            items = []
            for idx, elt in enumerate(values.elts):
                items.append(f"[{idx}]")
            return items
    return None


def _is_fixture_decorator(decorator):
    if isinstance(decorator, ast.Call):
        return _is_fixture_decorator(decorator.func)
    if isinstance(decorator, ast.Attribute):
        if decorator.attr != "fixture":
            return False
        value = decorator.value
        return (
            (isinstance(value, ast.Name) and value.id in ("pytest", "mark", "oxtest"))
            or (
                isinstance(value, ast.Attribute)
                and isinstance(value.value, ast.Name)
                and value.value.id in ("pytest", "oxtest")
                and value.attr == "mark"
            )
        )
    if isinstance(decorator, ast.Name):
        return decorator.id == "fixture"
    return False


def _extract_mark_name(decorator):
    if isinstance(decorator, ast.Call):
        return _extract_mark_name(decorator.func)
    if isinstance(decorator, ast.Attribute):
        attr = decorator.attr
        value = decorator.value
        if isinstance(value, ast.Name) and value.id == "mark":
            return attr
        if isinstance(value, ast.Attribute) and value.attr == "mark" and isinstance(value.value, ast.Name) and value.value.id in ("pytest", "oxtest"):
            return attr
    return None


def _safe_literal_eval(node):
    if isinstance(node, ast.Name):
        if node.id == "True":
            return True
        if node.id == "False":
            return False
        if node.id == "None":
            return None
        return getattr(__builtins__, node.id, None)
    if isinstance(node, ast.Attribute):
        value = _safe_literal_eval(node.value)
        if value is None:
            return None
        return getattr(value, node.attr, None)
    try:
        return ast.literal_eval(node)
    except Exception:
        return None


def _extract_mark_info(decorator):
    name = _extract_mark_name(decorator)
    if not name:
        return None
    if isinstance(decorator, ast.Call):
        args = [_safe_literal_eval(arg) for arg in decorator.args]
        kwargs = {kw.arg: _safe_literal_eval(kw.value) for kw in decorator.keywords if kw.arg is not None}
    else:
        args = []
        kwargs = {}
    return {"name": name, "args": args, "kwargs": kwargs}


def _collect_marks(decorators):
    marks = []
    for decorator in decorators:
        mark = _extract_mark_name(decorator)
        if mark:
            marks.append(mark)
    return marks


def _extract_extra_keywords(test_name, class_name, marks):
    keywords = {test_name}
    if class_name:
        keywords.add(class_name)
        keywords.add(f"{class_name}.{test_name}")
    for mark in marks:
        keywords.add(mark)
    return list(keywords)


def _discover_tests_in_function(func, class_name, class_marks):
    marks = _collect_marks(func.decorator_list) + class_marks
    names = [func.name]
    for decorator in func.decorator_list:
        param_names = _normalize_parametrize(decorator)
        if param_names:
            names = [f"{func.name}{param}" for param in param_names]
            break
    return [(name, marks, _extract_extra_keywords(name, class_name, marks)) for name in names]


def _discover_class_tests(class_node):
    tests = []
    class_marks = _collect_marks(class_node.decorator_list)
    for item in class_node.body:
        if isinstance(item, ast.FunctionDef) and item.name.startswith("test_"):
            param_names = None
            for decorator in item.decorator_list:
                param_names = _normalize_parametrize(decorator)
                if param_names:
                    break
            marks = _collect_marks(item.decorator_list) + class_marks
            if param_names:
                tests.extend([ (f"{class_node.name}.{item.name}{param}", marks, _extract_extra_keywords(f"{item.name}{param}", class_node.name, marks)) for param in param_names])
            else:
                tests.append((f"{class_node.name}.{item.name}", marks, _extract_extra_keywords(item.name, class_node.name, marks)))
    return tests


def discover(path):
    path = pathlib.Path(path)
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    tests = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name.startswith("test_"):
            tests.extend(_discover_tests_in_function(node, None, []))
        elif isinstance(node, ast.ClassDef) and node.name.startswith("Test"):
            tests.extend(_discover_class_tests(node))
    return [{"name": name, "marks": marks, "extra_keywords": extra_keywords} for name, marks, extra_keywords in tests]


def _is_fixture_function(func):
    return any(_is_fixture_decorator(d) for d in func.decorator_list)


def list_fixtures(path):
    path = pathlib.Path(path)
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    fixtures = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and _is_fixture_function(node):
            fixtures.append(node.name)
        elif isinstance(node, ast.ClassDef):
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and _is_fixture_function(item):
                    fixtures.append(f"{node.name}.{item.name}")
                    fixtures.append(item.name)
    return fixtures


def fixtures_per_test(path):
    fixtures = set(list_fixtures(path))
    path = pathlib.Path(path)
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    tests = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name.startswith("test_"):
            params = [arg.arg for arg in node.args.args if arg.arg in fixtures]
            tests.append({"name": node.name, "fixtures": params})
        elif isinstance(node, ast.ClassDef) and node.name.startswith("Test"):
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and item.name.startswith("test_"):
                    params = [arg.arg for arg in item.args.args if arg.arg in fixtures]
                    tests.append({"name": f"{node.name}.{item.name}", "fixtures": params})
    return tests


def load_module(path):
    path = pathlib.Path(path)
    _ensure_package_root(path)
    _ensure_oxtest_module()
    _ensure_pytest_module()
    module_name = f"oxtest_module_{path.stem}"
    spec = importlib.util.spec_from_file_location(module_name, str(path))
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def _apply_fixture(func, *args):
    if not func:
        return
    func(*args)


def _load_attr(obj, name):
    try:
        return getattr(obj, name)
    except AttributeError:
        return None


def _strip_param_id(name):
    if name.endswith("]") and "[" in name:
        return name[: name.index("[")]
    return name


def _extract_param_index(name):
    if name.endswith("]") and "[" in name:
        try:
            return int(name[name.rindex("[") + 1 : -1])
        except ValueError:
            return None
    return None


def _is_param_call(node):
    if not isinstance(node, ast.Call):
        return False
    func = node.func
    if isinstance(func, ast.Name):
        return func.id == "param"
    if isinstance(func, ast.Attribute):
        return func.attr == "param" and isinstance(func.value, ast.Name) and func.value.id in ("pytest", "oxtest")
    return False


def _literal_eval_node(node):
    if isinstance(node, ast.Call) and _is_param_call(node):
        values = [_literal_eval_node(arg) for arg in node.args]
        return values[0] if len(values) == 1 else tuple(values)
    if isinstance(node, ast.List):
        return [_literal_eval_node(elt) for elt in node.elts]
    if isinstance(node, ast.Tuple):
        return tuple(_literal_eval_node(elt) for elt in node.elts)
    try:
        return ast.literal_eval(node)
    except Exception:
        return None


def _extract_parametrize_info(decorator):
    if not isinstance(decorator, ast.Call):
        return None
    func = decorator.func
    if not (isinstance(func, ast.Attribute) and func.attr == "parametrize"):
        return None
    args = decorator.args
    if len(args) < 2:
        return None
    names_node = args[0]
    values_node = args[1]
    if not isinstance(names_node, ast.Constant) or not isinstance(names_node.value, str):
        return None
    names = [name.strip() for name in names_node.value.split(",") if name.strip()]
    values = _literal_eval_node(values_node)
    if not isinstance(values, (list, tuple)):
        return None
    return names, values


def _get_parametrize_kwargs(path, test_name):
    param_index = _extract_param_index(test_name)
    if param_index is None:
        return {}
    base_name = _strip_param_id(test_name)
    class_name = None
    func_name = base_name
    if "." in base_name:
        class_name, func_name = base_name.split(".", 1)

    path = pathlib.Path(path)
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    for node in tree.body:
        if class_name is None and isinstance(node, ast.FunctionDef) and node.name == func_name:
            for decorator in node.decorator_list:
                info = _extract_parametrize_info(decorator)
                if info is not None:
                    names, values = info
                    if param_index < 0 or param_index >= len(values):
                        raise IndexError("Parameter index out of range")
                    param_values = values[param_index]
                    if len(names) == 1:
                        return {names[0]: param_values}
                    return {name: value for name, value in zip(names, param_values)}
        elif class_name is not None and isinstance(node, ast.ClassDef) and node.name == class_name:
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and item.name == func_name:
                    for decorator in item.decorator_list:
                        info = _extract_parametrize_info(decorator)
                        if info is not None:
                            names, values = info
                            if param_index < 0 or param_index >= len(values):
                                raise IndexError("Parameter index out of range")
                            param_values = values[param_index]
                            if len(names) == 1:
                                return {names[0]: param_values}
                            return {name: value for name, value in zip(names, param_values)}
    return {}


def _get_test_marks(path, test_name):
    base_name = _strip_param_id(test_name)
    class_name = None
    func_name = base_name
    if "." in base_name:
        class_name, func_name = base_name.split(".", 1)

    path = pathlib.Path(path)
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    marks = []
    for node in tree.body:
        if class_name is None and isinstance(node, ast.FunctionDef) and node.name == func_name:
            for decorator in node.decorator_list:
                info = _extract_mark_info(decorator)
                if info is not None:
                    marks.append(info)
            return marks
        elif class_name is not None and isinstance(node, ast.ClassDef) and node.name == class_name:
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and item.name == func_name:
                    for decorator in item.decorator_list:
                        info = _extract_mark_info(decorator)
                        if info is not None:
                            marks.append(info)
                    return marks
    return marks


def _extract_terms(expr):
    return set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", expr))


def _ensure_package_root(path):
    path = pathlib.Path(path).resolve()
    package_dir = None
    current = path.parent
    while current != current.parent:
        if (current / "__init__.py").exists():
            package_dir = current
        current = current.parent

    if package_dir is not None:
        root = package_dir.parent
        if str(root) not in sys.path:
            sys.path.insert(0, str(root))


def _ensure_oxtest_module():
    if "oxtest" in sys.modules:
        return
    import types
    import importlib
    import warnings

    class _SkipTest(Exception):
        pass

    class _XFailed(Exception):
        pass

    class _Approx:
        def __init__(self, expected, rel=1e-6, abs=1e-12):
            self.expected = expected
            self.rel = rel
            self.abs = abs

        def __eq__(self, actual):
            if self.expected is None:
                return actual is None
            if isinstance(self.expected, (list, tuple)) and isinstance(actual, (list, tuple)):
                if len(self.expected) != len(actual):
                    return False
                return all(_Approx(exp, self.rel, self.abs) == act for exp, act in zip(self.expected, actual))
            try:
                expected = float(self.expected)
                actual = float(actual)
            except Exception:
                return self.expected == actual
            diff = abs(expected - actual)
            tolerance = self.abs + self.rel * abs(expected)
            return diff <= tolerance

        def __repr__(self):
            return f"approx({self.expected!r})"

    class _Mark:
        def __getattr__(self, name):
            def marker(*args, **kwargs):
                if len(args) == 1 and callable(args[0]) and not kwargs:
                    return args[0]
                def _inner(fn):
                    return fn
                return _inner
            return marker

    class _Param(tuple):
        def __new__(cls, args, **kwargs):
            obj = tuple.__new__(cls, args)
            obj._pytest_param = kwargs
            return obj

        def __repr__(self):
            if self._pytest_param:
                return f"oxtest.param({tuple(self)!r}, {self._pytest_param!r})"
            return tuple.__repr__(self)

    class _Raises:
        def __init__(self, expected, *args, **kwargs):
            self.expected = expected
            self.exception = None

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            if exc_type is None:
                raise AssertionError(f"DID NOT RAISE {self.expected}")
            if isinstance(exc_value, self.expected):
                self.exception = exc_value
                return True
            return False

    class _DeprecatedCall:
        def __init__(self, func=None, *args, **kwargs):
            self.func = func
            self.args = args
            self.kwargs = kwargs

        def __call__(self, *args, **kwargs):
            combined_args = args or self.args
            combined_kwargs = kwargs or self.kwargs
            with warnings.catch_warnings():
                warnings.simplefilter("always")
                result = self.func(*combined_args, **combined_kwargs)
            return result

        def __enter__(self):
            warnings.simplefilter("always")
            self._warning_manager = warnings.catch_warnings(record=True)
            self._records = self._warning_manager.__enter__()
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            self._warning_manager.__exit__(exc_type, exc_value, traceback)
            if exc_type is not None:
                return False
            if not any(issubclass(type(w.message), DeprecationWarning) for w in self._records):
                raise AssertionError("Deprecated call did not emit DeprecationWarning")
            return True

    class _Warns:
        def __init__(self, expected_warning, match=None):
            self.expected_warning = expected_warning
            self.match = match
            self._records = None

        def __enter__(self):
            self._warning_manager = warnings.catch_warnings(record=True)
            self._records = self._warning_manager.__enter__()
            warnings.simplefilter("always")
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            self._warning_manager.__exit__(exc_type, exc_value, traceback)
            if exc_type is not None:
                return False
            for record in self._records:
                if issubclass(record.message.__class__, self.expected_warning):
                    if self.match is None or self.match in str(record.message):
                        return True
            raise AssertionError(f"Did not warn with {self.expected_warning}")

    def fixture(func):
        func.__oxtest_fixture__ = True
        return func

    def approx(expected, rel=1e-6, abs=1e-12):
        return _Approx(expected, rel, abs)

    def fail(msg="", pytrace=True):
        raise AssertionError(msg)

    def skip(msg=""):
        raise _SkipTest(msg)

    def importorskip(module_name, minversion=None, reason=None):
        try:
            return importlib.import_module(module_name)
        except ImportError:
            raise _SkipTest(reason or f"skipped: {module_name} not available")

    def xfail(reason=""):
        raise _XFailed(reason)

    def exit(msg=""):
        raise SystemExit(msg)

    def main(args=None):
        return 0

    def param(*args, **kwargs):
        return _Param(args, **kwargs)

    def raises(expected, *args, **kwargs):
        if args or kwargs:
            raise TypeError("oxtest.raises does not support direct call invocation in this shim")
        return _Raises(expected)

    def deprecated_call(func=None, *args, **kwargs):
        if func is not None and callable(func):
            return _DeprecatedCall(func, *args, **kwargs)()
        return _DeprecatedCall(func, *args, **kwargs)

    def register_assert_rewrite(*args, **kwargs):
        return None

    def warns(expected_warning, match=None):
        return _Warns(expected_warning, match=match)

    def freeze_includes(*args, **kwargs):
        return None

    module = types.ModuleType("oxtest")
    module.fixture = fixture
    module.mark = _Mark()
    module.param = param
    module.approx = approx
    module.fail = fail
    module.skip = skip
    module.importorskip = importorskip
    module.xfail = xfail
    module.exit = exit
    module.main = main
    module.raises = raises
    module.deprecated_call = deprecated_call
    module.register_assert_rewrite = register_assert_rewrite
    module.warns = warns
    module.freeze_includes = freeze_includes
    module.SkipTest = _SkipTest
    module.XFailed = _XFailed
    sys.modules["oxtest"] = module


def _ensure_pytest_module():
    if "pytest" in sys.modules:
        return
    import types
    import importlib
    import warnings

    class _Approx:
        def __init__(self, expected, rel=1e-6, abs=1e-12):
            self.expected = expected
            self.rel = rel
            self.abs = abs

        def __eq__(self, actual):
            if self.expected is None:
                return actual is None
            if isinstance(self.expected, (list, tuple)) and isinstance(actual, (list, tuple)):
                if len(self.expected) != len(actual):
                    return False
                return all(_Approx(exp, self.rel, self.abs) == act for exp, act in zip(self.expected, actual))
            try:
                expected = float(self.expected)
                actual = float(actual)
            except Exception:
                return self.expected == actual
            diff = abs(expected - actual)
            tolerance = self.abs + self.rel * abs(expected)
            return diff <= tolerance

        def __repr__(self):
            return f"approx({self.expected!r})"

    class _Param(tuple):
        def __new__(cls, args, **kwargs):
            obj = tuple.__new__(cls, args)
            obj._pytest_param = kwargs
            return obj

        def __repr__(self):
            if self._pytest_param:
                return f"pytest.param({tuple(self)!r}, {self._pytest_param!r})"
            return tuple.__repr__(self)

    class _Raises:
        def __init__(self, expected, *args, **kwargs):
            self.expected = expected
            self.exception = None

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            if exc_type is None:
                raise AssertionError(f"DID NOT RAISE {self.expected}")
            if isinstance(exc_value, self.expected):
                self.exception = exc_value
                return True
            return False

    class _DeprecatedCall:
        def __init__(self, func=None, *args, **kwargs):
            self.func = func
            self.args = args
            self.kwargs = kwargs

        def __call__(self, *args, **kwargs):
            combined_args = args or self.args
            combined_kwargs = kwargs or self.kwargs
            with warnings.catch_warnings():
                warnings.simplefilter("always")
                result = self.func(*combined_args, **combined_kwargs)
            return result

        def __enter__(self):
            warnings.simplefilter("always")
            self._warning_manager = warnings.catch_warnings(record=True)
            self._records = self._warning_manager.__enter__()
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            self._warning_manager.__exit__(exc_type, exc_value, traceback)
            if exc_type is not None:
                return False
            if not any(issubclass(type(w.message), DeprecationWarning) for w in self._records):
                raise AssertionError("Deprecated call did not emit DeprecationWarning")
            return True

    class _Warns:
        def __init__(self, expected_warning, match=None):
            self.expected_warning = expected_warning
            self.match = match
            self._records = None

        def __enter__(self):
            self._warning_manager = warnings.catch_warnings(record=True)
            self._records = self._warning_manager.__enter__()
            warnings.simplefilter("always")
            return self

        def __exit__(self, exc_type, exc_value, traceback):
            caught = self._warning_manager.__exit__(exc_type, exc_value, traceback)
            if exc_type is not None:
                return False
            for record in self._records:
                if issubclass(record.message.__class__, self.expected_warning):
                    if self.match is None or self.match in str(record.message):
                        return True
            raise AssertionError(f"Did not warn with {self.expected_warning}")

    class _Mark:
        def __getattr__(self, name):
            def marker(*args, **kwargs):
                if len(args) == 1 and callable(args[0]) and not kwargs:
                    return args[0]
                def _inner(fn):
                    return fn
                return _inner
            return marker

    def approx(expected, rel=1e-6, abs=1e-12):
        return _Approx(expected, rel, abs)

    def fail(msg="", pytrace=True):
        raise AssertionError(msg)

    def skip(msg=""): 
        raise _SkipTest(msg)

    def importorskip(module_name, minversion=None, reason=None):
        try:
            return importlib.import_module(module_name)
        except ImportError:
            raise _SkipTest(reason or f"skipped: {module_name} not available")

    def xfail(reason=""):
        raise _XFailed(reason)

    def exit(msg=""):
        raise SystemExit(msg)

    def main(args=None):
        return 0

    def param(*args, **kwargs):
        return _Param(args, **kwargs)

    def raises(expected, *args, **kwargs):
        if args or kwargs:
            raise TypeError("pytest.raises does not support direct call invocation in this shim")
        return _Raises(expected)

    def deprecated_call(func=None, *args, **kwargs):
        if func is not None and callable(func):
            return _DeprecatedCall(func, *args, **kwargs)()
        return _DeprecatedCall(func, *args, **kwargs)

    def register_assert_rewrite(*args, **kwargs):
        return None

    def warns(expected_warning, match=None):
        return _Warns(expected_warning, match=match)

    def freeze_includes(*args, **kwargs):
        return None

    module = types.ModuleType("pytest")
    module.approx = approx
    module.fail = fail
    module.skip = skip
    module.importorskip = importorskip
    module.xfail = xfail
    module.exit = exit
    module.main = main
    module.param = param
    module.raises = raises
    module.deprecated_call = deprecated_call
    module.register_assert_rewrite = register_assert_rewrite
    module.warns = warns
    module.freeze_includes = freeze_includes
    module.SkipTest = _SkipTest
    module.XFailed = _XFailed
    module.mark = _Mark()
    sys.modules["pytest"] = module


def _resolve_fixture(module, fixture_name, fixture_names, cache, stack):
    if fixture_name in cache:
        return cache[fixture_name]
    if fixture_name in stack:
        raise RuntimeError(f"Circular fixture dependency: {' -> '.join(stack + [fixture_name])}")
    fixture = getattr(module, fixture_name, None)
    if fixture is None or not callable(fixture):
        raise KeyError(f"Fixture {fixture_name} not found")
    stack.append(fixture_name)
    try:
        sig = inspect.signature(fixture)
        kwargs = {}
        for param in sig.parameters.values():
            if param.name == "self":
                continue
            if param.name in fixture_names:
                kwargs[param.name] = _resolve_fixture(module, param.name, fixture_names, cache, stack)
        value = fixture(**kwargs)
        cache[fixture_name] = value
        return value
    finally:
        stack.pop()


def _call_test_with_fixtures(func, module, path, test_name):
    fixture_names = set(list_fixtures(path))
    marks = _get_test_marks(path, test_name)
    usefixtures = []
    filterwarnings = []
    skip_reason = None
    xfail_reason = None

    for mark in marks:
        if mark["name"] == "skip":
            skip_reason = mark["args"][0] if mark["args"] else mark["kwargs"].get("reason", "")
        elif mark["name"] == "skipif":
            condition = mark["args"][0] if mark["args"] else mark["kwargs"].get("condition", False)
            if condition:
                skip_reason = mark["kwargs"].get("reason", "")
        elif mark["name"] == "usefixtures":
            for arg in mark["args"]:
                if isinstance(arg, (list, tuple)):
                    usefixtures.extend(arg)
                elif isinstance(arg, str):
                    usefixtures.append(arg)
        elif mark["name"] == "xfail":
            xfail_reason = mark["kwargs"].get("reason", mark["args"][0] if mark["args"] else "")
        elif mark["name"] == "filterwarnings":
            filterwarnings.append((mark["args"], mark["kwargs"]))

    if skip_reason is not None:
        raise _SkipTest(skip_reason)

    parametrize_kwargs = _get_parametrize_kwargs(path, test_name)
    sig = inspect.signature(func)
    kwargs = {}
    for param in sig.parameters.values():
        if param.name == "self":
            continue
        if param.name in fixture_names:
            kwargs[param.name] = _resolve_fixture(module, param.name, fixture_names, {}, [])

    for fixture in usefixtures:
        if fixture not in kwargs:
            _resolve_fixture(module, fixture, fixture_names, {}, [])

    kwargs.update(parametrize_kwargs)

    def invoke():
        return func(**kwargs)

    if filterwarnings:
        with warnings.catch_warnings():
            for args, kw in filterwarnings:
                warnings.filterwarnings(*args, **kw)
            try:
                result = invoke()
            except Exception as err:
                if xfail_reason is not None:
                    raise _XFailed(xfail_reason or str(err))
                raise
            if xfail_reason is not None:
                raise _XFailed(xfail_reason or "expected xfail")
            return result
    else:
        try:
            result = invoke()
        except Exception as err:
            if xfail_reason is not None:
                raise _XFailed(xfail_reason or str(err))
            raise
        if xfail_reason is not None:
            raise _XFailed(xfail_reason or "expected xfail")
        return result


def match_keyword(expr, test_name, extra_names):
    if not expr:
        return True
    context = {}
    test_name_lower = test_name.lower()
    for term in _extract_terms(expr):
        if term in {"and", "or", "not", "True", "False"}:
            continue
        value = term.lower() in test_name_lower or any(term.lower() in extra.lower() for extra in extra_names)
        context[term] = value
    try:
        return bool(eval(expr, {"__builtins__": None}, context))
    except Exception:
        return False


def match_markexpr(expr, marks):
    if not expr:
        return True
    context = {mark: True for mark in marks}
    try:
        return bool(eval(expr, {"__builtins__": None}, context))
    except Exception:
        return False


def run_test(path, test_name):
    module = load_module(path)
    setup_module = _load_attr(module, "setup_module")
    teardown_module = _load_attr(module, "teardown_module")

    if "." in test_name:
        class_name, method_name = test_name.split(".", 1)
        base_method_name = _strip_param_id(method_name)
        cls = getattr(module, class_name)
        instance = cls()

        setup_class = _load_attr(cls, "setup_class")
        teardown_class = _load_attr(cls, "teardown_class")
        setup_method = _load_attr(instance, "setup_method")
        teardown_method = _load_attr(instance, "teardown_method")

        try:
            _apply_fixture(setup_module, module)
            _apply_fixture(setup_class, cls)
            _apply_fixture(setup_method, instance, base_method_name)
            func = getattr(instance, base_method_name)
            _call_test_with_fixtures(func, module, path, test_name)
            return True, ""
        except _SkipTest as err:
            return True, f"skipped: {err}"
        except _XFailed as err:
            return True, f"xfail: {err}"
        except Exception:
            return False, traceback.format_exc()
        finally:
            _apply_fixture(teardown_method, instance, base_method_name)
            _apply_fixture(teardown_class, cls)
            _apply_fixture(teardown_module, module)
    else:
        base_test_name = _strip_param_id(test_name)
        setup_function = _load_attr(module, "setup_function")
        teardown_function = _load_attr(module, "teardown_function")
        try:
            _apply_fixture(setup_module, module)
            _apply_fixture(setup_function, base_test_name)
            func = getattr(module, base_test_name)
            _call_test_with_fixtures(func, module, path, test_name)
            return True, ""
        except _SkipTest as err:
            return True, f"skipped: {err}"
        except _XFailed as err:
            return True, f"xfail: {err}"
        except Exception:
            return False, traceback.format_exc()
        finally:
            _apply_fixture(teardown_function, base_test_name)
            _apply_fixture(teardown_module, module)
"#;

pub fn discover_tests(path: &str, config: &RunConfig) -> Result<Vec<TestItem>> {
    let mut config = config.clone();
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.configure(&mut config);
    }

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
            .downcast::<pyo3::types::PyList>()
            .map_err(|err| anyhow!("Python fixtures_per_test returned unexpected value: {}", err))?;
        let mut usage = Vec::new();

        for item in list.iter() {
            let dict = item
                .downcast::<pyo3::types::PyDict>()
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
        let discover = module.getattr("discover")?;
        let result = discover.call1((path.to_str().unwrap(),))?;
        let list = result
            .downcast::<pyo3::types::PyList>()
            .map_err(|err| anyhow!("Python discovery returned unexpected value: {}", err))?;
        let mut items = Vec::new();

        for item in list.iter() {
            let dict = item
                .downcast::<pyo3::types::PyDict>()
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

pub fn run_tests(path: &str, config: RunConfig) -> Result<TestSummary> {
    let mut config = config;
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.configure(&mut config);
    }

    let tests = discover_tests(path, &config)?;
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

    Ok(TestSummary {
        results,
        passed,
        failed,
    })
}

fn run_tests_sequential(tests: Vec<TestItem>, config: &RunConfig) -> Result<Vec<TestResult>> {
    let mut results = Vec::new();
    let mut failures = 0;

    for test in tests {
        let result = run_test_item(&test)?;
        for plugin in plugin_registry().lock().unwrap().iter() {
            plugin.after_test(&test, &result);
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
                let result = run_test_item(&test)?;
                for plugin in &plugins {
                    plugin.after_test(&test, &result);
                }
                Ok(result)
            })
            .collect::<Result<Vec<_>, _>>()
    })
}

pub fn run_test_item(test: &TestItem) -> Result<TestResult> {
    for plugin in plugin_registry().lock().unwrap().iter() {
        plugin.before_test(test);
    }
    let (passed, output) = run_test(test)?;
    let result = TestResult {
        file: test.file.clone(),
        full_name: test.full_name.clone(),
        passed,
        output,
        marks: test.marks.clone(),
    };
    Ok(result)
}

fn run_test(test: &TestItem) -> Result<(bool, String)> {
    Python::with_gil(|py| {
        let module = PyModule::from_code(py, PY_HELPER, "oxtest_helper.py", "oxtest_helper")?;
        let run_test = module.getattr("run_test")?;
        let result = run_test.call1((test.file.to_str().unwrap(), test.full_name.as_str()))?;
        let tuple: (bool, String) = result.extract()?;
        Ok(tuple)
    })
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
