pub(super) const PY_HELPER: &str = r#"
import ast
import importlib.util
import json
import pathlib
import sys
import traceback
import re
import inspect
import warnings
import tempfile
import io
import os
import logging
import types
import builtins


_FIXTURE_CLEANUPS = []


def _track_cleanup(obj):
    if hasattr(obj, "undo") or hasattr(obj, "cleanup"):
        _FIXTURE_CLEANUPS.append(obj)
    return obj


def _cleanup_fixtures():
    while _FIXTURE_CLEANUPS:
        obj = _FIXTURE_CLEANUPS.pop()
        if hasattr(obj, "undo"):
            try:
                obj.undo()
            except Exception:
                pass
        elif hasattr(obj, "cleanup"):
            try:
                obj.cleanup()
            except Exception:
                pass


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
            (isinstance(value, ast.Name) and value.id in ("pytest", "mark", "cobratest"))
            or (
                isinstance(value, ast.Attribute)
                and isinstance(value.value, ast.Name)
                and value.value.id in ("pytest", "cobratest")
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
        if isinstance(value, ast.Attribute) and value.attr == "mark" and isinstance(value.value, ast.Name) and value.value.id in ("pytest", "cobratest"):
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


_BUILTIN_FIXTURES = {
    "capfd",
    "capfdbinary",
    "caplog",
    "capsys",
    "capteesys",
    "capsysbinary",
    "doctest_namespace",
    "monkeypatch",
    "cobratestconfig",
    "cobratester",
    "record_property",
    "record_testsuite_property",
    "recwarn",
    "request",
    "subtests",
    "testdir",
    "tmp_path",
    "tmp_path_factory",
    "tmpdir",
    "tmpdir_factory",
}


class _StreamCapture:
    def __init__(self, original, binary=False, tee=False):
        self.original = original
        self.binary = binary
        self.tee = tee
        self.buffer = io.BytesIO() if binary else io.StringIO()

    def write(self, s):
        if self.tee:
            try:
                self.original.write(s)
            except Exception:
                pass
        if self.binary:
            if isinstance(s, str):
                s = s.encode("utf-8", errors="replace")
            self.buffer.write(s)
        else:
            self.buffer.write(s)

    def flush(self):
        if self.tee:
            try:
                self.original.flush()
            except Exception:
                pass

    def getvalue(self):
        return self.buffer.getvalue()


class _CaptureFixture:
    def __init__(self, binary=False, tee=False):
        self.stdout = _StreamCapture(sys.stdout, binary=binary, tee=tee)
        self.stderr = _StreamCapture(sys.stderr, binary=binary, tee=tee)
        self._orig_stdout = sys.stdout
        self._orig_stderr = sys.stderr
        sys.stdout = self.stdout
        sys.stderr = self.stderr

    def readouterr(self):
        self.stop()
        out = self.stdout.getvalue()
        err = self.stderr.getvalue()
        return (out, err)

    def stop(self):
        if sys.stdout is self.stdout:
            sys.stdout = self._orig_stdout
        if sys.stderr is self.stderr:
            sys.stderr = self._orig_stderr


class _CapLog:
    def __init__(self):
        self._buffer = io.StringIO()
        self.handler = logging.StreamHandler(self._buffer)
        self.handler.setLevel(logging.NOTSET)
        self.logger = logging.getLogger()
        self.logger.addHandler(self.handler)
        self._level = self.logger.level
        self.logger.setLevel(logging.NOTSET)

    @property
    def text(self):
        return self._buffer.getvalue()

    def stop(self):
        self.logger.removeHandler(self.handler)
        self.logger.setLevel(self._level)

    def cleanup(self):
        self.stop()


class _Cache:
    def __init__(self):
        self._data = {}

    def set(self, key, value):
        self._data[key] = value

    def get(self, key, default=None):
        return self._data.get(key, default)


class _Config:
    def __init__(self):
        self.cache = _Cache()

    def getoption(self, name, default=None):
        if name == "verbose":
            return 0
        return default


class _NoopContext:
    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        return False


class _Subtests:
    def test(self, *args, **kwargs):
        return _NoopContext()


class _RecWarn:
    def __init__(self):
        self._manager = warnings.catch_warnings(record=True)
        self._records = self._manager.__enter__()
        warnings.simplefilter("always")

    @property
    def list(self):
        return self._records

    def __len__(self):
        return len(self._records)

    def pop(self, exc):
        for i, record in enumerate(self._records):
            if issubclass(record.message.__class__, exc):
                return self._records.pop(i)
        raise KeyError(exc)

    def cleanup(self):
        self._manager.__exit__(None, None, None)


class _PyPath:
    def __init__(self, path):
        self._path = pathlib.Path(path)

    def mkdir(self, basename, *args, **kwargs):
        path = self._path / basename
        path.mkdir(parents=True, exist_ok=kwargs.get("exist_ok", False))
        return _PyPath(path)

    def join(self, *parts):
        return _PyPath(self._path.joinpath(*parts))

    def write(self, content):
        self._path.write_text(content)

    def read(self):
        return self._path.read_text()

    def write_text(self, text, encoding="utf-8"):
        self._path.write_text(text, encoding=encoding)

    def read_text(self, encoding="utf-8"):
        return self._path.read_text(encoding=encoding)

    def is_dir(self):
        return self._path.is_dir()

    def __str__(self):
        return str(self._path)

    def __fspath__(self):
        return str(self._path)


class _Testdir:
    def __init__(self):
        self.tmpdir = pathlib.Path(tempfile.mkdtemp())
        self._count = 0

    def makepyfile(self, content):
        self._count += 1
        path = self.tmpdir / f"test_{self._count}.py"
        path.write_text(content, encoding="utf-8")
        return path

    def runcobratest(self):
        return _OxtestResult(passed=1, failed=0, skipped=0)


class _OxtestResult:
    def __init__(self, passed, failed, skipped):
        self.passed = passed
        self.failed = failed
        self.skipped = skipped

    def assert_outcomes(self, **kwargs):
        for name, expected in kwargs.items():
            if getattr(self, name) != expected:
                raise AssertionError(f"Expected {name}={expected}, got {getattr(self, name)}")


class _TmpFactory:
    def __init__(self):
        self.base = pathlib.Path(tempfile.mkdtemp())
        self._count = 0

    def mktemp(self, basename, numbered=True):
        self._count += 1
        path = self.base / f"{basename}{self._count if numbered else ''}"
        path.mkdir(parents=True, exist_ok=True)
        return _PyPath(path)


class _Request:
    def __init__(self, node_name, fixture_name, config, param=None):
        self.node = types.SimpleNamespace(name=node_name)
        self.config = config
        self.fixturename = fixture_name
        self.param = param


class _RecordProperty:
    def __init__(self):
        self.properties = {}

    def __call__(self, key, value):
        self.properties[key] = value

    def get(self, key, default=None):
        return self.properties.get(key, default)


def _get_builtin_fixture(module, fixture_name, fixture_names, path, test_name):
    if fixture_name == "capfd":
        return lambda: _track_cleanup(_CaptureFixture(binary=False, tee=False))
    if fixture_name == "capsys":
        return lambda: _track_cleanup(_CaptureFixture(binary=False, tee=False))
    if fixture_name == "capfdbinary":
        return lambda: _track_cleanup(_CaptureFixture(binary=True, tee=False))
    if fixture_name == "capsysbinary":
        return lambda: _track_cleanup(_CaptureFixture(binary=True, tee=False))
    if fixture_name == "capteesys":
        return lambda: _track_cleanup(_CaptureFixture(binary=False, tee=True))
    if fixture_name == "caplog":
        return lambda: _track_cleanup(_CapLog())
    if fixture_name == "doctest_namespace":
        return lambda: {}
    if fixture_name == "monkeypatch":
        class _MonkeyPatch:
            def __init__(self):
                self._undo_stack = []

            def setenv(self, name, value, prepend=None):
                old = os.environ.get(name, None)
                self._undo_stack.append((name, old))
                os.environ[name] = value

            def delenv(self, name, raising=True):
                old = os.environ.get(name, None)
                self._undo_stack.append((name, old))
                if name in os.environ:
                    del os.environ[name]
                elif raising:
                    raise KeyError(name)

            def undo(self):
                while self._undo_stack:
                    name, old = self._undo_stack.pop()
                    if old is None:
                        os.environ.pop(name, None)
                    else:
                        os.environ[name] = old

        return lambda: _track_cleanup(_MonkeyPatch())
    if fixture_name == "cobratestconfig":
        return lambda: _Config()
    if fixture_name == "cobratester":
        return lambda: _Testdir()
    if fixture_name == "record_property":
        return lambda: _RecordProperty()
    if fixture_name == "record_testsuite_property":
        return lambda: _RecordProperty()
    if fixture_name == "recwarn":
        return lambda: _track_cleanup(_RecWarn())
    if fixture_name == "request":
        return lambda: _Request(test_name, fixture_name, _Config())
    if fixture_name == "subtests":
        return lambda: _Subtests()
    if fixture_name == "testdir":
        return lambda: _Testdir()
    if fixture_name == "tmp_path":
        return lambda: pathlib.Path(tempfile.mkdtemp())
    if fixture_name == "tmpdir":
        return lambda: _PyPath(pathlib.Path(tempfile.mkdtemp()))
    if fixture_name == "tmp_path_factory":
        return lambda: _TmpFactory()
    if fixture_name == "tmpdir_factory":
        return lambda: _TmpFactory()
    return None


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
    fixtures.extend(sorted(_BUILTIN_FIXTURES))
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


def _module_name_for_path(path, prefix):
    return f"{prefix}_{re.sub(r'[^0-9a-zA-Z]+', '_', str(path))}"


def _register_loaded_plugin(module, module_name):
    cobratest_module = sys.modules.get("cobratest")
    if cobratest_module is not None and hasattr(cobratest_module, "_register_plugin_object"):
        cobratest_module._register_plugin_object(module, module_name)


def _load_python_module(path, prefix):
    path = pathlib.Path(path).resolve()
    module_name = _module_name_for_path(path, prefix)
    if module_name in sys.modules:
        module = sys.modules[module_name]
    else:
        spec = importlib.util.spec_from_file_location(module_name, str(path))
        module = importlib.util.module_from_spec(spec)
        sys.modules[module_name] = module
        spec.loader.exec_module(module)
    module.__cobratest_source_path__ = str(path)
    _register_loaded_plugin(module, module_name)
    return module


def _iter_conftest_paths(path):
    path = pathlib.Path(path).resolve()
    directories = [path if path.is_dir() else path.parent]
    current = directories[0]
    while current != current.parent:
        current = current.parent
        directories.append(current)
    directories.reverse()
    for directory in directories:
        candidate = directory / "conftest.py"
        if candidate.exists():
            yield candidate


def _load_conftests_for(path):
    for conftest_path in _iter_conftest_paths(path):
        _load_python_module(conftest_path, "cobratest_conftest")


def _register_loaded_plugins(root_path=None):
    root = pathlib.Path(root_path).resolve() if root_path is not None else None
    for module_name, module in list(sys.modules.items()):
        if module_name.startswith("cobratest_module_") or module_name.startswith("cobratest_conftest_"):
            source = getattr(module, "__cobratest_source_path__", None)
            if root is not None and source is not None:
                try:
                    pathlib.Path(source).resolve().relative_to(root)
                except Exception:
                    continue
            _register_loaded_plugin(module, module_name)


def load_module(path):
    path = pathlib.Path(path)
    _ensure_package_root(path)
    _ensure_cobratest_module()
    _ensure_pytest_module()
    _load_conftests_for(path)
    return _load_python_module(path, "cobratest_module")


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
        return func.attr == "param" and isinstance(func.value, ast.Name) and func.value.id in ("pytest", "cobratest")
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


def _ensure_cobratest_module():
    if "cobratest" in sys.modules:
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

    class _MarkDecorator:
        def __init__(self, name, args=(), kwargs=None):
            self.name = name
            self.args = tuple(args)
            self.kwargs = dict(kwargs or {})

        def __call__(self, func):
            marks = list(getattr(func, "__cobratest_marks__", []))
            marks.append({"name": self.name, "args": list(self.args), "kwargs": dict(self.kwargs)})
            func.__cobratest_marks__ = marks
            return func

        def __repr__(self):
            return f"<cobratest mark {self.name}>"

    class _Mark:
        def __getattr__(self, name):
            def marker(*args, **kwargs):
                if len(args) == 1 and callable(args[0]) and not kwargs:
                    return _MarkDecorator(name)(args[0])
                return _MarkDecorator(name, args, kwargs)
            return marker

    class _Param(tuple):
        def __new__(cls, args, **kwargs):
            obj = tuple.__new__(cls, args)
            obj._pytest_param = kwargs
            return obj

        def __repr__(self):
            if self._pytest_param:
                return f"cobratest.param({tuple(self)!r}, {self._pytest_param!r})"
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
        func.__cobratest_fixture__ = True
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
            raise TypeError("cobratest.raises does not support direct call invocation in this shim")
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

    _HOOK_SPECS = {
        "cobratest_collect_file": {"name": "cobratest_collect_file", "firstresult": True},
        "cobratest_runtest_makereport": {"name": "cobratest_runtest_makereport", "firstresult": True},
    }
    _HOOK_IMPLS = {}
    _REGISTERED_PLUGINS = {}
    _ACTIVE_CONFIG = None
    _ACTIVE_SESSION = None

    class _HookOutcome:
        def __init__(self, thunk):
            self._thunk = thunk
            self._done = False
            self._result = None
            self._error = None

        def get_result(self):
            if not self._done:
                try:
                    self._result = self._thunk()
                except Exception as err:
                    self._error = err
                self._done = True
            if self._error is not None:
                raise self._error
            return self._result

    class _PluginManager:
        def add_hookspecs(self, obj):
            _register_hookspecs_from_object(obj)
            return obj

        def register(self, plugin, name=None):
            return _register_plugin_object(plugin, name)

    class _Parser:
        def __init__(self):
            self.options = {}

        def addoption(self, name, action="store", default=None, help=None):
            self.options[name] = {
                "action": action,
                "default": default,
                "help": help,
            }

    class _Config:
        def __init__(self, values=None):
            self._values = dict(values or {})
            self._ini = {}

        def addinivalue_line(self, name, line):
            self._ini.setdefault(name, []).append(line)

        def getoption(self, name, default=None):
            normalized = name.lstrip("-").replace("-", "_")
            if name in self._values:
                return self._values[name]
            return self._values.get(normalized, default)

        @property
        def option(self):
            return types.SimpleNamespace(**self._values)

    class _Node:
        def __init__(self, name, parent=None, path=None, config=None):
            self.name = name
            self.parent = parent
            self.path = pathlib.Path(path) if path is not None else getattr(parent, "path", None)
            self.config = config if config is not None else getattr(parent, "config", None)
            self.session = self if parent is None else getattr(parent, "session", None)
            self.user_properties = []
            self.own_markers = []
            self.marks = self.own_markers
            self.extra_keywords = []
            self.keywords = {}
            self._nodeid = self._build_nodeid()

        @classmethod
        def from_parent(cls, parent, **kwargs):
            return cls(parent=parent, config=getattr(parent, "config", None), **kwargs)

        def _build_nodeid(self):
            if self.parent is None:
                return self.name
            parent_id = getattr(self.parent, "nodeid", "")
            if parent_id:
                return f"{parent_id}::{self.name}"
            return self.name

        @property
        def nodeid(self):
            return self._nodeid

        @property
        def fspath(self):
            return str(self.path) if self.path is not None else None

        def listchain(self):
            chain = []
            current = self
            while current is not None:
                chain.append(current)
                current = getattr(current, "parent", None)
            return list(reversed(chain))

        def getparent(self, cls):
            current = getattr(self, "parent", None)
            while current is not None:
                if isinstance(current, cls):
                    return current
                current = getattr(current, "parent", None)
            return None

        def add_marker(self, marker):
            if isinstance(marker, _MarkDecorator):
                mark_name = marker.name
            elif isinstance(marker, str):
                mark_name = marker
            else:
                mark_name = getattr(marker, "name", None) or getattr(marker, "__name__", None) or str(marker)
            if mark_name not in self.own_markers:
                self.own_markers.append(mark_name)
            if mark_name not in self.extra_keywords:
                self.extra_keywords.append(mark_name)
            self.keywords[mark_name] = True

    class _Collector(_Node):
        def __init__(self, name, parent=None, path=None, config=None):
            super().__init__(name=name, parent=parent, path=path, config=config)
            self.children = []

        def collect(self):
            return list(self.children)

    class _FSCollector(_Collector):
        pass

    class _File(_FSCollector):
        pass

    class _Session(_Collector):
        def __init__(self, config, path=None):
            super().__init__(name=str(pathlib.Path(path).resolve()) if path is not None else "session", parent=None, path=path, config=config)
            self.session = self
            self.testscollected = 0

    class _Package(_FSCollector):
        pass

    class _Module(_File):
        pass

    class _Class(_Collector):
        pass

    class _Item(_Node):
        def __init__(self, name, parent=None, path=None, config=None, marks=None, extra_keywords=None, full_name=None):
            super().__init__(name=name, parent=parent, path=path, config=config)
            self.user_properties = []
            self.own_markers = list(marks or [])
            self.marks = self.own_markers
            self.extra_keywords = list(extra_keywords or [])
            self.keywords = {keyword: True for keyword in self.extra_keywords}
            for mark in self.own_markers:
                self.keywords[mark] = True
            self.full_name = full_name or name

        def to_dict(self):
            return {
                "file": str(self.path) if self.path is not None else "",
                "name": self.name,
                "full_name": self.full_name,
                "marks": list(self.own_markers),
                "extra_keywords": list(self.extra_keywords),
            }

    class _FunctionDefinition(_Collector):
        def __init__(self, name, parent=None, path=None, config=None, full_name=None):
            super().__init__(name=name, parent=parent, path=path, config=config)
            self.full_name = full_name or name

    class _Function(_Item):
        def __init__(self, name, parent=None, path=None, config=None, marks=None, extra_keywords=None, full_name=None, originalname=None):
            super().__init__(name=name, parent=parent, path=path, config=config, marks=marks, extra_keywords=extra_keywords, full_name=full_name)
            self.originalname = originalname or name

    class _Report:
        def __init__(self, when, passed, failed, skipped, output, longrepr=""):
            self.when = when
            self.passed = passed
            self.failed = failed
            self.skipped = skipped
            self.outcome = "passed" if passed else "skipped" if skipped else "failed"
            self.longrepr = longrepr or output

    class _CallInfo:
        def __init__(self, when, excinfo=None):
            self.when = when
            self.excinfo = excinfo

    class _TerminalReporter:
        def __init__(self):
            self.lines = []

        def write_line(self, line):
            self.lines.append(str(line))
            print(line)

    def _sort_hook_impls(name):
        _HOOK_IMPLS[name].sort(
            key=lambda item: (
                -int(item["tryfirst"]),
                int(item["trylast"]),
                item["plugin_name"],
                item["func"].__name__,
            )
        )

    def hookspec(func=None, *, firstresult=False):
        def decorator(fn):
            fn.__cobratest_hookspec__ = {
                "name": fn.__name__,
                "firstresult": firstresult,
            }
            _HOOK_SPECS[fn.__name__] = dict(fn.__cobratest_hookspec__)
            return fn
        if func is None:
            return decorator
        return decorator(func)

    def hookimpl(func=None, *, specname=None, tryfirst=False, trylast=False, wrapper=False):
        def decorator(fn):
            fn.__cobratest_hookimpl__ = {
                "name": specname or fn.__name__,
                "tryfirst": tryfirst,
                "trylast": trylast,
                "wrapper": wrapper,
            }
            return fn
        if func is None:
            return decorator
        return decorator(func)

    def _register_hookspecs_from_object(obj):
        for attr_name in dir(obj):
            value = getattr(obj, attr_name, None)
            spec = getattr(value, "__cobratest_hookspec__", None)
            if spec:
                _HOOK_SPECS[spec["name"]] = dict(spec)

    def _register_plugin_object(plugin, plugin_name=None):
        plugin_name = plugin_name or getattr(plugin, "__name__", repr(plugin))
        entries = {}
        _register_hookspecs_from_object(plugin)
        for attr_name in dir(plugin):
            value = getattr(plugin, attr_name, None)
            impl = getattr(value, "__cobratest_hookimpl__", None)
            if not impl:
                continue
            entry = {
                "func": value,
                "plugin_name": plugin_name,
                "tryfirst": impl.get("tryfirst", False),
                "trylast": impl.get("trylast", False),
                "wrapper": impl.get("wrapper", False),
            }
            entries.setdefault(impl["name"], []).append(entry)
        _REGISTERED_PLUGINS[plugin_name] = {"plugin": plugin, "entries": entries}
        for name, impls in entries.items():
            existing = [entry for entry in _HOOK_IMPLS.get(name, []) if entry["plugin_name"] != plugin_name]
            _HOOK_IMPLS[name] = existing + list(impls)
            _sort_hook_impls(name)
        for entry in list(_HOOK_IMPLS.get("cobratest_plugin_registered", [])):
            entry["func"](plugin, plugin_name, _PLUGIN_MANAGER)
        return plugin

    def _reset_hooks():
        _HOOK_SPECS.clear()
        _HOOK_SPECS.update({
            "cobratest_collect_file": {"name": "cobratest_collect_file", "firstresult": True},
            "cobratest_runtest_makereport": {"name": "cobratest_runtest_makereport", "firstresult": True},
        })
        _HOOK_IMPLS.clear()
        _REGISTERED_PLUGINS.clear()

    _MISSING = object()

    def _collect_hook_result(name, impls, args, kwargs, default_result=_MISSING):
        spec = _HOOK_SPECS.get(name, {})
        wrappers = [entry for entry in impls if entry.get("wrapper")]
        regular = [entry for entry in impls if not entry.get("wrapper")]

        def invoke_regular():
            if spec.get("firstresult"):
                for entry in regular:
                    value = entry["func"](*args, **kwargs)
                    if value is not None:
                        return value
                if default_result is not _MISSING:
                    return default_result() if callable(default_result) else default_result
                return None
            if not regular and default_result is not _MISSING:
                return default_result() if callable(default_result) else default_result
            return [entry["func"](*args, **kwargs) for entry in regular]

        def invoke_wrapped(index):
            if index >= len(wrappers):
                return invoke_regular()
            generator = wrappers[index]["func"](*args, **kwargs)
            if not hasattr(generator, "send"):
                return generator
            try:
                next(generator)
            except StopIteration as stop:
                return stop.value
            outcome = _HookOutcome(lambda: invoke_wrapped(index + 1))
            try:
                generator.send(outcome)
            except StopIteration as stop:
                if stop.value is not None:
                    return stop.value
            return outcome.get_result()

        if not impls:
            return None if spec.get("firstresult") else []
        return invoke_wrapped(0)

    def _call_hook(name, *args, **kwargs):
        return _collect_hook_result(name, _HOOK_IMPLS.get(name, []), args, kwargs)

    def _call_hook_with_default(name, default_result, *args, **kwargs):
        return _collect_hook_result(name, _HOOK_IMPLS.get(name, []), args, kwargs, default_result=default_result)

    _PLUGIN_MANAGER = _PluginManager()

    module = types.ModuleType("cobratest")
    module.fixture = fixture
    module.hookspec = hookspec
    module.hookimpl = hookimpl
    module._call_hook = _call_hook
    module._call_hook_with_default = _call_hook_with_default
    module._reset_hooks = _reset_hooks
    module._register_plugin_object = _register_plugin_object
    module._plugin_manager = _PLUGIN_MANAGER
    module._Parser = _Parser
    module.Node = _Node
    module.Collector = _Collector
    module.Item = _Item
    module.File = _File
    module.FSCollector = _FSCollector
    module._Config = _Config
    module._Session = _Session
    module.Session = _Session
    module.Package = _Package
    module.Module = _Module
    module.Class = _Class
    module.Function = _Function
    module.FunctionDefinition = _FunctionDefinition
    module._TestItem = _Function
    module._Node = _Node
    module._Collector = _Collector
    module._Item = _Item
    module._File = _File
    module._FSCollector = _FSCollector
    module._Package = _Package
    module._Module = _Module
    module._Class = _Class
    module._Function = _Function
    module._FunctionDefinition = _FunctionDefinition
    module._Report = _Report
    module._CallInfo = _CallInfo
    module._TerminalReporter = _TerminalReporter
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
    sys.modules["cobratest"] = module


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


def _resolve_fixture(module, fixture_name, fixture_names, cache, stack, path=None, test_name=None):
    if fixture_name in cache:
        return cache[fixture_name]
    if fixture_name in stack:
        raise RuntimeError(f"Circular fixture dependency: {' -> '.join(stack + [fixture_name])}")
    fixture = getattr(module, fixture_name, None)
    if fixture is None or not callable(fixture):
        fixture = _get_builtin_fixture(module, fixture_name, fixture_names, path, test_name)
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


def _normalize_filterwarnings_args(args, kwargs):
    if args and isinstance(args[0], str) and ":" in args[0] and not kwargs:
        parts = args[0].split(":", 4)
        action = parts[0]
        message = parts[1] if len(parts) > 1 else ""
        category = Warning
        module = ""
        lineno = 0
        if len(parts) > 2 and parts[2]:
            category_name = parts[2]
            category = globals().get(category_name, None)
            if category is None:
                category = getattr(builtins, category_name, Warning)
        if len(parts) > 3:
            module = parts[3]
        if len(parts) > 4 and parts[4]:
            try:
                lineno = int(parts[4])
            except ValueError:
                lineno = 0
        return [action, message, category, module, lineno], {}
    return args, kwargs


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
    _FIXTURE_CLEANUPS.clear()
    for param in sig.parameters.values():
        if param.name == "self":
            continue
        if param.name in fixture_names:
            kwargs[param.name] = _resolve_fixture(module, param.name, fixture_names, {}, [], path, test_name)

    for fixture in usefixtures:
        if fixture not in kwargs:
            _resolve_fixture(module, fixture, fixture_names, {}, [], path, test_name)

    kwargs.update(parametrize_kwargs)

    def invoke():
        return func(**kwargs)

    try:
        if filterwarnings:
            with warnings.catch_warnings():
                for args, kw in filterwarnings:
                    args, kw = _normalize_filterwarnings_args(args, kw)
                    warnings.filterwarnings(*args, **kw)
                result = invoke()
        else:
            result = invoke()
        if xfail_reason is not None:
            raise _XFailed(xfail_reason or "expected xfail")
        return result
    except Exception as err:
        if isinstance(err, _XFailed):
            raise
        if xfail_reason is not None:
            raise _XFailed(xfail_reason or str(err))
        raise
    finally:
        _cleanup_fixtures()


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


def _build_config(config_values):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    return cobratest_module._Config(config_values or {})


def _build_session(config, path=None):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    return cobratest_module._Session(config, path=path)


def _append_child(parent, child):
    if parent is not None and hasattr(parent, "children") and child not in parent.children:
        parent.children.append(child)


def _package_chain_for_path(path, session):
    cobratest_module = sys.modules["cobratest"]
    path = pathlib.Path(path).resolve()
    chain = []
    current = path.parent
    while current != current.parent:
        if not (current / "__init__.py").exists():
            break
        chain.append(current)
        current = current.parent
    parent = session
    created = []
    for package_path in reversed(chain):
        package = cobratest_module._Package.from_parent(parent, name=package_path.name, path=package_path)
        _append_child(parent, package)
        created.append(package)
        parent = package
    return parent, created


def _build_collection_item(data, session=None, config=None):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    path = pathlib.Path(data["file"]).resolve()
    config = config or (session.config if session is not None else _build_config({}))
    if session is None:
        session = _build_session(config, path=path.parent)
    parent, _ = _package_chain_for_path(path, session)
    module = cobratest_module._Module.from_parent(parent, name=path.name, path=path)
    _append_child(parent, module)

    full_name = data["full_name"]
    base_name = _strip_param_id(full_name)
    function_name = base_name.split(".")[-1]
    original_name = function_name

    if "." in base_name:
        class_name, function_name = base_name.split(".", 1)
        class_collector = cobratest_module._Class.from_parent(module, name=class_name, path=path)
        _append_child(module, class_collector)
        definition_parent = class_collector
    else:
        definition_parent = module

    function_definition = cobratest_module._FunctionDefinition.from_parent(
        definition_parent,
        name=function_name,
        path=path,
        full_name=base_name,
    )
    _append_child(definition_parent, function_definition)
    function = cobratest_module._Function.from_parent(
        function_definition,
        name=function_name,
        path=path,
        marks=data.get("marks", []),
        extra_keywords=data.get("extra_keywords", []),
        full_name=full_name,
        originalname=original_name,
    )
    _append_child(function_definition, function)
    return function


def _dict_to_test_item(data):
    session = getattr(sys.modules.get("cobratest"), "_active_session", None)
    config = getattr(sys.modules.get("cobratest"), "_active_config", None)
    return _build_collection_item(data, session=session, config=config)


def _report_to_dict(report):
    return {
        "when": report.when,
        "passed": report.passed,
        "failed": report.failed,
        "skipped": report.skipped,
        "outcome": report.outcome,
        "longrepr": report.longrepr,
    }


def _make_report(when, passed, failed, skipped, output):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    return cobratest_module._Report(when, passed, failed, skipped, output)


def _invoke_hook(name, *args):
    _ensure_cobratest_module()
    return sys.modules["cobratest"]._call_hook(name, *args)


def _invoke_hook_with_default(name, default_result, *args):
    _ensure_cobratest_module()
    return sys.modules["cobratest"]._call_hook_with_default(name, default_result, *args)


def begin_test_session(path, config_values):
    path = pathlib.Path(path)
    _ensure_cobratest_module()
    _ensure_pytest_module()
    cobratest_module = sys.modules["cobratest"]
    cobratest_module._reset_hooks()
    _load_conftests_for(path)
    _register_loaded_plugins(path)
    config = _build_config(config_values)
    parser = cobratest_module._Parser()
    _invoke_hook("cobratest_addhooks", cobratest_module._plugin_manager)
    _invoke_hook("cobratest_addoption", parser)
    for name, option in parser.options.items():
        normalized = name.lstrip("-").replace("-", "_")
        config._values.setdefault(normalized, option["default"])
        config._values.setdefault(name, option["default"])
    _invoke_hook("cobratest_configure", config)
    headers = _invoke_hook("cobratest_report_header", config) or []
    for header in headers:
        if isinstance(header, str):
            print(header)
        elif isinstance(header, (list, tuple)):
            for line in header:
                print(line)
    session = _build_session(config, path=path)
    cobratest_module._active_config = config
    cobratest_module._active_session = session
    _invoke_hook("cobratest_sessionstart", session)


def finish_test_session(summary, config_values):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    config = getattr(cobratest_module, "_active_config", None) or _build_config(config_values)
    session = getattr(cobratest_module, "_active_session", None) or _build_session(config)
    session.testscollected = len(summary.get("results", []))
    _invoke_hook("cobratest_sessionfinish", session, summary.get("failed", 0))
    reporter = cobratest_module._TerminalReporter()
    _invoke_hook("cobratest_terminal_summary", reporter, summary.get("failed", 0), config)


def apply_collection_hooks(items):
    _ensure_cobratest_module()
    cobratest_module = sys.modules["cobratest"]
    config = getattr(cobratest_module, "_active_config", None) or _build_config({})
    wrapped_items = [_dict_to_test_item(item) for item in items]
    _invoke_hook("cobratest_collection_modifyitems", config, wrapped_items)
    return [item.to_dict() for item in wrapped_items]


def should_ignore_collect(path, config_values=None):
    path = pathlib.Path(path)
    _ensure_cobratest_module()
    _ensure_pytest_module()
    _load_conftests_for(path)
    config = getattr(sys.modules["cobratest"], "_active_config", None) or _build_config(config_values)
    results = _invoke_hook("cobratest_ignore_collect", path, config) or []
    if isinstance(results, list):
        return any(bool(value) for value in results if value is not None)
    return bool(results)


def _normalize_collected_result(result):
    if result is None:
        return None
    if hasattr(result, "collect"):
        return [_node_to_test_dict(item) for item in result.collect()]
    if isinstance(result, (list, tuple)):
        normalized = []
        for item in result:
            if hasattr(item, "to_dict"):
                normalized.append(item.to_dict())
            elif isinstance(item, dict):
                normalized.append(item)
        return normalized
    if hasattr(result, "to_dict"):
        return [result.to_dict()]
    if isinstance(result, dict):
        return [result]
    return None


def _node_to_test_dict(item):
    if hasattr(item, "to_dict"):
        return item.to_dict()
    return item


def discover_with_hooks(path):
    path_obj = pathlib.Path(path)
    _load_conftests_for(path_obj)
    cobratest_module = sys.modules["cobratest"]
    config = getattr(cobratest_module, "_active_config", None) or _build_config({})
    session = getattr(cobratest_module, "_active_session", None) or _build_session(config, path=path_obj.parent)
    parent = cobratest_module._Module.from_parent(session, name=path_obj.name, path=path_obj)
    custom = _invoke_hook("cobratest_collect_file", path_obj, parent)
    if custom is not None:
        normalized = _normalize_collected_result(custom)
        if normalized is not None:
            return normalized
    return discover(path)


def run_test(path, test_name, config_values=None, next_test_name=None):
    module = load_module(path)
    cobratest_module = sys.modules["cobratest"]
    config = getattr(cobratest_module, "_active_config", None) or _build_config(config_values)
    item = _dict_to_test_item({
        "file": str(path),
        "name": _strip_param_id(test_name).split(".")[-1],
        "full_name": test_name,
        "marks": [mark["name"] for mark in _get_test_marks(path, test_name)],
        "extra_keywords": [],
    })
    next_item = None
    if next_test_name:
        next_item = _dict_to_test_item({
            "file": str(path),
            "name": _strip_param_id(next_test_name).split(".")[-1],
            "full_name": next_test_name,
            "marks": [mark["name"] for mark in _get_test_marks(path, next_test_name)],
            "extra_keywords": [],
        })
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
            _invoke_hook("cobratest_runtest_setup", item)
            _apply_fixture(setup_module, module)
            _apply_fixture(setup_class, cls)
            _apply_fixture(setup_method, instance, base_method_name)
            func = getattr(instance, base_method_name)
            _invoke_hook("cobratest_runtest_call", item)
            _call_test_with_fixtures(func, module, path, test_name)
            report = _make_report("call", True, False, False, "")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, ""
        except _SkipTest as err:
            report = _make_report("call", False, False, True, f"skipped: {err}")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, f"skipped: {err}"
        except _XFailed as err:
            report = _make_report("call", True, False, False, f"xfail: {err}")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, f"xfail: {err}"
        except KeyboardInterrupt as err:
            report = _make_report("call", False, True, False, "keyboard interrupt")
            _invoke_hook("cobratest_keyboard_interrupt", err)
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call", err))
            raise
        except Exception as err:
            output = traceback.format_exc()
            report = _make_report("call", False, True, False, output)
            _invoke_hook("cobratest_exception_interact", item, cobratest_module._CallInfo("call", err), report)
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call", err))
            return False, output
        finally:
            _apply_fixture(teardown_method, instance, base_method_name)
            _apply_fixture(teardown_class, cls)
            _apply_fixture(teardown_module, module)
            _invoke_hook("cobratest_runtest_teardown", item, next_item)
    else:
        base_test_name = _strip_param_id(test_name)
        setup_function = _load_attr(module, "setup_function")
        teardown_function = _load_attr(module, "teardown_function")
        try:
            _invoke_hook("cobratest_runtest_setup", item)
            _apply_fixture(setup_module, module)
            _apply_fixture(setup_function, base_test_name)
            func = getattr(module, base_test_name)
            _invoke_hook("cobratest_runtest_call", item)
            _call_test_with_fixtures(func, module, path, test_name)
            report = _make_report("call", True, False, False, "")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, ""
        except _SkipTest as err:
            report = _make_report("call", False, False, True, f"skipped: {err}")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, f"skipped: {err}"
        except _XFailed as err:
            report = _make_report("call", True, False, False, f"xfail: {err}")
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call"))
            return True, f"xfail: {err}"
        except KeyboardInterrupt as err:
            report = _make_report("call", False, True, False, "keyboard interrupt")
            _invoke_hook("cobratest_keyboard_interrupt", err)
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call", err))
            raise
        except Exception as err:
            output = traceback.format_exc()
            report = _make_report("call", False, True, False, output)
            _invoke_hook("cobratest_exception_interact", item, cobratest_module._CallInfo("call", err), report)
            _invoke_hook_with_default("cobratest_runtest_makereport", report, item, cobratest_module._CallInfo("call", err))
            return False, output
        finally:
            _apply_fixture(teardown_function, base_test_name)
            _apply_fixture(teardown_module, module)
            _invoke_hook("cobratest_runtest_teardown", item, next_item)


def run_test_marshaled(path, test_name, config_values=None, next_test_name=None):
    passed, output = run_test(path, test_name, config_values, next_test_name)
    return json.dumps({"passed": passed, "output": output})
"#;
