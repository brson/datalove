#!/usr/bin/env python3
# Sum benchmark - sum integers from 1 to 30,000,000 into an arbitrary-precision int.

def sum_to(limit: int) -> int:
    total = 0
    for i in range(1, limit + 1):
        total += i
    return total

result = sum_to(30_000_000)
print(result)
