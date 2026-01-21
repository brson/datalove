#!/usr/bin/env python3
# Fibonacci benchmark - compute fib(40) iteratively

def fib(n: int) -> int:
    if n < 2:
        return n
    a, b = 0, 1
    for _ in range(2, n + 1):
        a, b = b, a + b
    return b

result = fib(40)
print(result)
