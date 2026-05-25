# cobratest.fixture examples for built-in fixtures


def test_capfd_example(capfd):
    print("hello")
    out, err = capfd.readouterr()
    assert out == "hello\n"


def test_capfdbinary_example(capfdbinary):
    print("hello")
    out, err = capfdbinary.readouterr()
    assert out == b"hello\n"


def test_caplog_example(caplog):
    import logging

    logging.getLogger().info("test log")
    assert "test log" in caplog.text


def test_capsys_example(capsys):
    print("hello")
    out, err = capsys.readouterr()
    assert out == "hello\n"


def test_capsysbinary_example(capsysbinary):
    print("hello")
    out, err = capsysbinary.readouterr()
    assert out == b"hello\n"


def test_config_cache_example(cobratestconfig):
    cobratestconfig.cache.set("example/value", 42)
    assert cobratestconfig.cache.get("example/value", None) == 42


def test_doctest_namespace_example(doctest_namespace):
    import math

    doctest_namespace["math"] = math
    assert "math" in doctest_namespace


def test_monkeypatch_example(monkeypatch):
    monkeypatch.setenv("MY_VAR", "123")
    import os

    assert os.getenv("MY_VAR") == "123"


def test_cobratestconfig_example(cobratestconfig):
    assert cobratestconfig.getoption("verbose") >= 0


def test_cobratester_example(cobratester):
    cobratester.makepyfile("def test_pass(): pass")
    result = cobratester.runcobratest()
    result.assert_outcomes(passed=1)


def test_record_property_example(record_property):
    record_property("key", "value")
    assert record_property.get("key") == "value"


def test_record_testsuite_property_example(record_testsuite_property):
    record_testsuite_property("suite_key", "suite_value")


def test_recwarn_example(recwarn):
    import warnings

    warnings.warn("hello", UserWarning)
    assert len(recwarn) == 1
    w = recwarn.pop(UserWarning)
    assert str(w.message) == "hello"


def test_request_example(request):
    assert request.node.name == "test_request_example"


def test_subtests_example(subtests):
    for i in range(3):
        with subtests.test(msg="iteration", i=i):
            assert i < 3


def test_testdir_example(testdir):
    testdir.makepyfile("def test_pass(): pass")
    result = testdir.runcobratest()
    result.assert_outcomes(passed=1)


def test_tmp_path_example(tmp_path):
    d = tmp_path / "sub"
    d.mkdir()
    f = d / "hello.txt"
    f.write_text("content")
    assert f.read_text() == "content"


def test_tmp_path_factory_example(tmp_path_factory):
    path = tmp_path_factory.mktemp("data")
    assert path.is_dir()


def test_tmpdir_example(tmpdir):
    f = tmpdir.mkdir("sub").join("hello.txt")
    f.write("content")
    assert f.read() == "content"


def test_tmpdir_factory_example(tmpdir_factory):
    path = tmpdir_factory.mktemp("data")
    assert path.is_dir()
