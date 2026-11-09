import re


def slugify(text):
    text = text.lower().strip()
    text = re.sub(r"[^a-z0-9]+", "-", text)
    return text.strip("-")


def truncate(text, n):
    if len(text) <= n:
        return text
    return text[: n - 3] + "..."


def count_words(text):
    return len(text.split())
