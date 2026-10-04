#!/usr/bin/env python3
# Word frequency benchmark - count the words of a generated text and report the
# most frequent. See wordfreq.dfs for the shape of the text, which every
# implementation generates identically from the same seed.
#
# Prints (words, distinct words, checksum of the top 20 by count).

from collections import Counter

WORDS = 2_000_000
TOP = 20
MASK = 0xFFFFFFFF


def main():
    syllables = ["ka", "lo", "mi", "ra", "te", "su", "no", "vi",
                 "de", "pa", "zu", "ri", "go", "ne", "ba", "to"]
    vocab = [syllables[i & 15] + syllables[(i >> 4) & 15] + syllables[i >> 8]
             for i in range(4096)]

    # The generator is inlined; a call per draw would mostly measure calls.
    state = 12345
    parts = []
    for i in range(WORDS):
        state = (state * 1664525 + 1013904223) & MASK
        a = (state >> 16) % 4096
        state = (state * 1664525 + 1013904223) & MASK
        b = (state >> 16) % 4096
        word = vocab[min(a, b)]
        state = (state * 1664525 + 1013904223) & MASK
        if (state >> 16) % 10 == 0:
            word = word.upper()
        parts.append(word)
        parts.append("\n" if i % 12 == 11 else " ")
    text = "".join(parts)

    counts = Counter(word.lower() for word in text.split())

    # By count descending, then by word.
    ranked = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))
    checksum = 0
    for word, count in ranked[:TOP]:
        checksum = (checksum * 31 + count) & MASK
        for byte in word.encode():
            checksum = (checksum * 31 + byte) & MASK

    print((WORDS, len(counts), checksum))


main()
