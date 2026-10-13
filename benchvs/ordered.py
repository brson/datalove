#!/usr/bin/env python3
# Ordered benchmark - generic ordered-collection algorithms at four element
# types: a binary heap used as a heapsort, a check that the result is sorted,
# a lower-bound binary search for a stream of probes, and a count of distinct
# elements. See ordered.dfs. Generic by duck typing: every element type here
# has `<` and `==`, a pair as a tuple. Prints the sum of the per-type
# checksums.

ITERATIONS = 200
SIZE = 1000

seed = 74755


# AWFY's Random: a 16-bit linear congruential generator.
def next_random():
    global seed
    seed = ((seed * 1309) + 13849) & 65535
    return seed


def heap_push(h, elem):
    h.append(elem)
    i = len(h) - 1
    while i > 0:
        parent = (i - 1) >> 1
        if not h[i] < h[parent]:
            break
        h[i], h[parent] = h[parent], h[i]
        i = parent


def heap_pop(h):
    n = len(h)
    if n == 0:
        return None
    last = n - 1
    h[0], h[last] = h[last], h[0]
    least = h.pop()
    i = 0
    while True:
        left = i * 2 + 1
        if left >= last:
            break
        child = left
        right = left + 1
        if right < last and h[right] < h[left]:
            child = right
        if not h[child] < h[i]:
            break
        h[i], h[child] = h[child], h[i]
        i = child
    return least


def heapsort(elems):
    heap = []
    while elems:
        heap_push(heap, elems.pop())
    out = []
    while True:
        elem = heap_pop(heap)
        if elem is None:
            break
        out.append(elem)
    return out


def out_of_order(xs):
    count = 0
    for i in range(1, len(xs)):
        if xs[i] < xs[i - 1]:
            count += 1
    return count


def lower_bound(xs, elem):
    low = 0
    high = len(xs)
    while low < high:
        mid = low + ((high - low) >> 1)
        if xs[mid] < elem:
            low = mid + 1
        else:
            high = mid
    return low


def search_all(xs, probes):
    total = 0
    n = len(xs)
    for probe in probes:
        at = lower_bound(xs, probe)
        total += at
        if at < n and xs[at] == probe:
            total += 1
    return total


def distinct(xs):
    if not xs:
        return 0
    count = 1
    for i in range(1, len(xs)):
        if xs[i - 1] < xs[i]:
            count += 1
    return count


def checksum(elems, probes):
    ordered = heapsort(elems)
    return out_of_order(ordered) + search_all(ordered, probes) + distinct(ordered)


# Three letters from 'a' to 'h', one for each three of the low nine bits.
def word(r):
    return chr(97 + (r & 7)) + chr(97 + ((r >> 3) & 7)) + chr(97 + ((r >> 6) & 7))


def ints():
    return [next_random() % 500 for _ in range(SIZE)]


def floats():
    return [(next_random() % 500) * 0.25 for _ in range(SIZE)]


def words():
    return [word(next_random()) for _ in range(SIZE)]


def pairs():
    xs = []
    for _ in range(SIZE):
        r = next_random()
        xs.append((word(r >> 2), r & 3))
    return xs


def benchmark():
    total = 0
    int_probes = ints()
    total += checksum(ints(), int_probes)
    float_probes = floats()
    total += checksum(floats(), float_probes)
    word_probes = words()
    total += checksum(words(), word_probes)
    pair_probes = pairs()
    total += checksum(pairs(), pair_probes)
    return total


total = 0
for _ in range(ITERATIONS):
    total = (total + benchmark()) & 0xFFFFFFFF
print(total)
