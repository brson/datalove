#!/usr/bin/env python3
# Sieve benchmark - count the primes up to 5000 with a sieve of Eratosthenes,
# many times over. From Are We Fast Yet (github.com/smarr/are-we-fast-yet),
# whose Sieve counts 669 each time. Prints the sum of the counts.

ITERATIONS = 10000


def sieve(flags, size):
    prime_count = 0
    for i in range(2, size + 1):
        if flags[i - 1]:
            prime_count += 1
            k = i + i
            while k <= size:
                flags[k - 1] = False
                k += i
    return prime_count


def benchmark():
    flags = [True] * 5000
    return sieve(flags, 5000)


total = 0
for _ in range(ITERATIONS):
    total += benchmark()
print(total)
