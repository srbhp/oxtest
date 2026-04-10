def test_hello_example():
    assert 1 == 1


# failed test
def inc(x):
    return x + 1


def test_answer():
    assert inc(3) == 5
