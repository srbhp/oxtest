# oxtest.fixture examples for built-in fixtures


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


def test_config_cache_example(oxtestconfig):
    oxtestconfig.cache.set("example/value", 42)
    assert oxtestconfig.cache.get("example/value", None) == 42


def test_doctest_namespace_example(doctest_namespace):
    import math

    doctest_namespace["math"] = math
    assert "math" in doctest_namespace


def test_monkeypatch_example(monkeypatch):
    monkeypatch.setenv("MY_VAR", "123")
    import os

    assert os.getenv("MY_VAR") == "123"


def test_oxtestconfig_example(oxtestconfig):
    assert oxtestconfig.getoption("verbose") >= 0


def test_oxtester_example(oxtester):
    oxtester.makepyfile("def test_pass(): pass")
    result = oxtester.runoxtest()
    result.assert_outcomes(passed=1)


def test_record_property_example(record_property):
    record_property("key", "value")
