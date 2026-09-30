# /// script
# requires-python = ">=3.10"
# dependencies = ["lizard==1.24.0"]
# ///
"""Lizard's cyclomatic complexity check, with its Rust reader corrected.

Lizard counts `||` as a logical operator wherever it appears, and `where` as
a branch. In Rust `||` also opens a closure without parameters — every
`use_signal(|| false)` of a component would add a path — and a `where`
clause constrains generics without branching. This keeps `||` only where it
joins two operands and drops `where`, then runs lizard with the arguments
given, unchanged: `uv run scripts/cyclomatic.py -l rust -C 15 -w crates`.
"""

import sys

import lizard

# What may end an operand: an identifier or literal, or a closing bracket or
# `?`. Keywords that open an expression do not.
OPENING_KEYWORDS = {"move", "return", "in", "async", "else", "break", "yield"}
CLOSING_PUNCTUATION = {")", "]", "}", "?"}


def ends_operand(token):
    if token is None:
        return False
    if token in CLOSING_PUNCTUATION:
        return True
    if token in OPENING_KEYWORDS:
        return False
    first = token[0]
    return first.isalnum() or first in "_\"'"


class RustClosuresAreNotBranches:
    """Hides closure `||` and `where` from the condition counter."""

    # Just before the condition counter, the last of lizard's processors.
    ordering_index = 4

    def __call__(self, tokens, reader):
        if reader.language_names != ["rust"]:
            yield from tokens
            return
        previous = None
        for token in tokens:
            if token == "||" and not ends_operand(previous):
                token = "| |"
            elif token == "where":
                token = "where_clause"
            if token != "\n":
                previous = token
            yield token


def extensions(names):
    return default_extensions(list(names) + [RustClosuresAreNotBranches()])


default_extensions = lizard.get_extensions
lizard.get_extensions = extensions

if __name__ == "__main__":
    sys.exit(lizard.main(sys.argv))
