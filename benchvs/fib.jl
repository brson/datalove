#!/usr/bin/env julia
# Fibonacci benchmark - compute fib(38) by naive recursion.
# Uses checked arithmetic to match datalove's checked u32 operators.

function fib(n::UInt32)::UInt32
    if n < 2
        return n
    end
    return Base.checked_add(fib(Base.checked_sub(n, UInt32(1))), fib(Base.checked_sub(n, UInt32(2))))
end

result = fib(UInt32(38))
println(result)
