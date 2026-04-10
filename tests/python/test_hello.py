import oxtest


def test_hello_example():
    assert 1 == 1


# failed test
def inc(x):
    return x + 1


def test_answer():
    assert inc(3) == 4


# fixture


class Fruit:
    def __init__(self, name):
        self.name = name

    def __eq__(self, other):
        return self.name == other.name


@oxtest.fixture
def my_fruit():
    return Fruit("apple")


@oxtest.fixture
def fruit_basket(my_fruit):
    return [Fruit("banana"), my_fruit]


def test_my_fruit_in_basket(my_fruit, fruit_basket):
    assert my_fruit in fruit_basket
