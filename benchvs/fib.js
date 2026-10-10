// Fibonacci benchmark - compute fib(38) by naive recursion.

function fib(n) {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

print(fib(38));
