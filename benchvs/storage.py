#!/usr/bin/env python3
# Storage benchmark - build a tree of arrays seven levels deep, four children
# to a node, whose leaves are arrays of 1 to 10 empty slots, and count the
# nodes, many times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose tree has 5461 nodes each time.
# Prints the sum of the counts.

ITERATIONS = 8000


class Random:
    def __init__(self):
        self.seed = 74755

    def next(self):
        self.seed = ((self.seed * 1309) + 13849) & 65535
        return self.seed


def build_tree_depth(depth, random, count):
    count[0] += 1
    if depth == 1:
        return [None] * (random.next() % 10 + 1)
    arr = [None] * 4
    for i in range(4):
        arr[i] = build_tree_depth(depth - 1, random, count)
    return arr


def benchmark():
    count = [0]
    build_tree_depth(7, Random(), count)
    return count[0]


total = 0
for _ in range(ITERATIONS):
    total += benchmark()
print(total)
