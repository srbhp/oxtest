import warnings

import oxtest


# oxtest.mark.filterwarnings
@oxtest.mark.filterwarnings("ignore:api v1 is deprecated")
def test_filter_warnings():
    warnings.warn("api v1 is deprecated", DeprecationWarning)
    assert True


# oxtest.mark.parametrize
@oxtest.mark.parametrize("test_input,expected", [("3+5", 8), ("2+4", 6), ("6*9", 54)])
def test_eval(test_input, expected):
    assert eval(test_input) == expected


# oxtest.mark.skip
@oxtest.mark.skip(reason="no way of currently testing this")
def test_the_unknown():
    pass


# oxtest.mark.skipif
@oxtest.mark.skipif(1 > 0, reason="skipping because 1 is greater than 0")
def test_conditional_skip():
    pass


# oxtest.mark.usefixtures
@oxtest.fixture
def cleandir():
    # setup code
    yield
    # teardown code


@oxtest.mark.usefixtures("cleandir")
class TestDirectoryStuff:
    def test_item(self):
        assert True


# oxtest.mark.xfail
@oxtest.mark.xfail(reason="known bug")
def test_expected_failure():
    assert 0 == 1


# Custom marks
@oxtest.mark.slow
def test_slow_operation():
    import time

    time.sleep(0.1)
    assert True
