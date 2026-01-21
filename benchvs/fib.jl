#!/usr/bin/env julia
# Fibonacci benchmark - compute fib(40) iteratively

function fib(n::Int)::BigInt
    if n < 2
        return BigInt(n)
    end
    a, b = BigInt(0), BigInt(1)
    for _ in 2:n
        a, b = b, a + b
    end
    return b
end

result = fib(40)
println(result)
