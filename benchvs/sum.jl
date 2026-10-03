#!/usr/bin/env julia
# Sum benchmark - sum integers from 1 to 1,000,000 into an arbitrary-precision int.

function sum_to(limit::Int)::BigInt
    total = BigInt(0)
    for i in 1:limit
        total += i
    end
    return total
end

result = sum_to(1_000_000)
println(result)
