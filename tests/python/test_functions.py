"""Examples of cobratest-compatible helper functions supported by cobratest.

This file contains runnable examples for safe usage and commented examples
for helper functions which are generally meant to demonstrate the API.
"""

import warnings

import cobratest

# Example usage of cobratest helpers:
#
# cobratest.approx(0.3)
# cobratest.fail("failure message")
# cobratest.skip("reason")
# cobratest.importorskip("module_name")
# cobratest.xfail("expected failure")
# cobratest.exit("bye")
# cobratest.main(["-q"])
# cobratest.param(1, id="one")
# with cobratest.raises(ValueError):
#     raise ValueError("boom")
# with cobratest.deprecated_call():
#     warnings.warn("deprecated", DeprecationWarning)
# cobratest.register_assert_rewrite("some_module")
# with cobratest.warns(DeprecationWarning):
#     warnings.warn("deprecated", DeprecationWarning)
# cobratest.freeze_includes("module_name")


def test_approx_example():
    assert 0.1 + 0.2 == cobratest.approx(0.3)


def test_fail_example():
    with cobratest.raises(AssertionError):
        cobratest.fail("boom")


def test_skip_example():
    with cobratest.raises(Exception):
        cobratest.skip("skip example")


def test_importorskip_example():
    assert cobratest.importorskip("sys") is not None


def test_xfail_example():
    with cobratest.raises(Exception):
        cobratest.xfail("expected failure example")


def test_exit_example():
    with cobratest.raises(SystemExit):
        cobratest.exit("bye")


@cobratest.mark.parametrize(
    "value, expected",
    [
        cobratest.param(1, 1, id="one"),
        cobratest.param(2, 2, id="two"),
    ],
)
def test_param_example(value, expected):
    assert value == expected


def test_raises_example():
    with cobratest.raises(ValueError):
        raise ValueError("boom")


def test_deprecated_call_example():
    def old_function():
        warnings.warn("deprecated", DeprecationWarning)
        return 1

    result = cobratest.deprecated_call(old_function)
    assert result == 1


def test_register_assert_rewrite_and_freeze_includes_example():
    assert cobratest.register_assert_rewrite("some_module") is None
    assert cobratest.freeze_includes("some_module") is None


def test_warns_example():
    with cobratest.warns(DeprecationWarning):
        warnings.warn("deprecated", DeprecationWarning)


@cobratest.fixture
def my_fruit_fixture():
    return "apple"


@cobratest.mark.usefixtures("my_fruit_fixture")
def test_usefixtures_example():
    assert True


@cobratest.mark.skip(reason="skip example")
def test_skip_example_mark():
    assert False


@cobratest.mark.skipif(True, reason="skipif example")
def test_skipif_example_mark():
    assert False


@cobratest.mark.xfail(reason="expected failure example")
def test_xfail_example_mark():
    raise AssertionError("boom")


@cobratest.mark.custom
def test_custom_mark_example():
    assert True
