# Fuzzing

```sh
cargo +nightly fuzz run xml_roundtrip -- -max_total_time=300
```

| Target | What it asserts |
|---|---|
| `xml_parse` | the parser never panics, hangs, or overflows the stack |
| `xml_roundtrip` | **for any input the parser accepts, re-serialising reproduces it byte for byte** — and reparsing our own output is idempotent |
| `header` | the outer header and VariantDictionary survive arbitrary bytes; every accessor is panic-free on a header that parsed but is nonsense |
| `container` | whole-file parsing, with KDF limits tightened so the fuzzer isn't spending its budget inside Argon2 |

`xml_roundtrip` is the valuable one. It encodes the property the entire project
rests on: a save must not change bytes it was not asked to change. Everything
else is a panic hunt.

## What it found

Four real bugs, none of which code review had caught. Three were silent
data-corruption paths, not crashes.

| Minimised input | Bug |
|---|---|
| `<a/></x><b/>` | **Silent truncation.** A stray end tag ended the epilogue loop early, so this parsed as `<a/>` — eight bytes discarded with no error. In a vault: entries missing on open, then destroyed for good by the next save. |
| `<r><b\x0bx=""></b></r>` | Inter-attribute whitespace was normalised to one space, and `str::trim_start` was accepting vertical tab, form feed and Unicode spaces that XML does not permit — a parser-differential hazard on top of the rewrite. |
| `<b></b\n>` | Whitespace inside end tags was dropped. |
| `<b ><b></b>>t</b>` | **Aliasing bug.** A start tag's pending whitespace was held in a field on the parser struct; since parsing children recurses through the same function, a nested element consumed its parent's copy. |

The last one is the instructive one: the fix was not "preserve more whitespace"
but "stop keeping per-element state on `self`". After it, the run went from
25 thousand executions before a crash to 6.5 million clean.

That prompted enumerating every whitespace position in the start-tag grammar
exhaustively rather than discovering them one crash at a time:

```text
<name WS1 attr WS2 = WS3 "value" WS1 attr … WS4 >
</name WS5 >
```

All five are now covered by `every_whitespace_position_in_the_grammar_round_trips`.

## Status

| Target | Executions | Failures |
|---|---|---|
| `xml_roundtrip` | 6,489,280 | 0 |
| `header` | 5,969,932 | 0 |
| `xml_parse` | 1,730,849 | 0 |
| `container` | 676,225 | 0 |

Every fixed bug has a regression test in `crates/vardo-kdbx/tests/`, so they
stay fixed without needing the fuzzer to rediscover them.
