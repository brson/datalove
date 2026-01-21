#!/usr/bin/env julia
# Prime counting benchmark - count primes up to 10,000

function is_prime(n::Int)::Bool
    if n < 2
        return false
    end
    if n == 2
        return true
    end
    if n % 2 == 0
        return false
    end
    d = 3
    while d * d <= n
        if n % d == 0
            return false
        end
        d += 2
    end
    return true
end

function count_primes(limit::Int)::Int
    count = 0
    for n in 2:limit
        if is_prime(n)
            count += 1
        end
    end
    return count
end

result = count_primes(10_000)
println(result)
