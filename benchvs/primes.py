#!/usr/bin/env python3
# Prime counting benchmark - count primes up to 4,000,000 by trial division.

def is_prime(n: int) -> bool:
    if n < 2:
        return False
    if n == 2:
        return True
    if n % 2 == 0:
        return False
    d = 3
    while d * d <= n:
        if n % d == 0:
            return False
        d += 2
    return True

def count_primes(limit: int) -> int:
    count = 0
    for n in range(2, limit + 1):
        if is_prime(n):
            count += 1
    return count

result = count_primes(4_000_000)
print(result)
