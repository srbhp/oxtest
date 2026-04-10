"""Examples of oxtest-compatible helper functions supported by oxtest.

This file contains runnable examples for safe usage and commented examples
for helper functions which are generally meant to demonstrate the API.
"""

import warnings

import oxtest

# Example usage of oxtest helpers:
#
# oxtest.approx(0.3)
# oxtest.fail("failure message")
# oxtest.skip("reason")
# oxtest.importorskip("module_name")
# oxtest.xfail("expected failure")
# oxtest.exit("bye")
# oxtest.main(["-q"])
# oxtest.param(1, id="one")
# with oxtest.raises(ValueError):
#     raise ValueError("boom")
# with oxtest.deprecated_call():
#     warnings.warn("deprecated", DeprecationWarning)
# oxtest.register_assert_rewrite("some_module")
# with oxtest.warns(DeprecationWarning):
#     warnings.warn("deprecated", DeprecationWarning)
# oxtest.freeze_includes("module_name")


def test_approx_example():
    assert 0.1 + 0.2 == oxtest.approx(0.3)


def test_fail_example():
    with oxtest.raises(AssertionError):
        oxtest.fail("boom")


def test_skip_example():
    with oxtest.raises(Exception):
        oxtest.skip("skip example")


def test_importorskip_example():
    assert oxtest.importorskip("sys") is not None


def test_xfail_example():
    with oxtest.raises(Exception):
        oxtest.xfail("expected failure example")


def test_exit_example():
    with oxtest.raises(SystemExit):
        oxtest.exit("bye")


@oxtest.mark.parametrize(
    "value, expected",
    [
        oxtest.param(1, 1, id="one"),
        oxtest.param(2, 2, id="two"),
    ],
)
def test_param_example(value, expected):
    assert value == expected


def test_raises_example():
    with oxtest.raises(ValueError):
        raise ValueError("boom")


def test_deprecated_call_example():
    def old_function():
        warnings.warn("deprecated", DeprecationWarning)
        return 1

    result = oxtest.deprecated_call(old_function)
    assert result == 1


def test_register_assert_rewrite_and_freeze_includes_example():
    assert oxtest.register_assert_rewrite("some_module") is None
    assert oxtest.freeze_includes("some_module") is None


def test_warns_example():
    with oxtest.warns(DeprecationWarning):
        warnings.warn("deprecated", DeprecationWarning)


@oxtest.fixture
def my_fruit_fixture():
    return "apple"


@oxtest.mark.usefixtures("my_fruit_fixture")
def test_usefixtures_example():
    assert True


@oxtest.mark.skip(reason="skip example")
def test_skip_example_mark():
    assert False


@oxtest.mark.skipif(True, reason="skipif example")
def test_skipif_example_mark():
    assert False


@oxtest.mark.xfail(reason="expected failure example")
def test_xfail_example_mark():
    raise AssertionError("boom")


@oxtest.mark.custom
def test_custom_mark_example():
    assert True
