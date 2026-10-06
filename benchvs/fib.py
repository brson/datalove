#!/usr/bin/env python3
# Fibonacci benchmark - compute fib(38) by naive recursion.

def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

result = fib(38)
print(result)
