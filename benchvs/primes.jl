#!/usr/bin/env julia
# Prime counting benchmark - count primes up to 4,000,000 by trial division.
# Uses wrapping UInt32 arithmetic to match datalove's wrapping u32 intrinsics.

function is_prime(n::UInt32)::Bool
    if n < 2
        return false
    end
    if n == 2
        return true
    end
    if n % UInt32(2) == 0
        return false
    end
    d = UInt32(3)
    while d * d <= n
        if n % d == 0
            return false
        end
        d += UInt32(2)
    end
    return true
end

function count_primes(limit::UInt32)::UInt32
    count = UInt32(0)
    n = UInt32(2)
    while n <= limit
        if is_prime(n)
            count += UInt32(1)
        end
        n += UInt32(1)
    end
    return count
end

result = count_primes(UInt32(4_000_000))
println(result)
