import argparse

from calc import add, sub, mul, div

OPS = {"add": add, "sub": sub, "mul": mul, "div": div}


def is_palindrome(s):
    s = "".join(ch.lower() for ch in s if ch.isalnum())
    return s == s[::-1]


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tinycalc")
    parser.add_argument("op", choices=sorted(OPS))
    parser.add_argument("a", type=float)
    parser.add_argument("b", type=float)
    parser.add_argument("--precision", type=int, default=6)
    args = parser.parse_args(argv)
    result = OPS[args.op](args.a, args.b)
    print(f"{result:.{args.precision}f}")


if __name__ == "__main__":
    main()
