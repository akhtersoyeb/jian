import unittest

from utils.strings import slugify, truncate, count_words


class TestStrings(unittest.TestCase):
    def test_slugify(self):
        self.assertEqual(slugify("Hello, World!"), "hello-world")

    def test_truncate_short(self):
        self.assertEqual(truncate("hello", 10), "hello")

    def test_truncate_long(self):
        self.assertEqual(truncate("hello world", 8), "hello...")

    def test_count_words(self):
        self.assertEqual(count_words("a b  c"), 3)


if __name__ == "__main__":
    unittest.main()
