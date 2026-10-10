#!/usr/bin/env julia
# Sieve benchmark - count the primes up to 5000 with a sieve of Eratosthenes,
# many times over. From Are We Fast Yet (github.com/smarr/are-we-fast-yet),
# whose Sieve counts 669 each time. Prints the sum of the counts.

const ITERATIONS = 10000

function sieve(flags::Vector{Bool}, size::Int)::Int
    prime_count = 0
    for i in 2:size
        if flags[i]
            prime_count += 1
            k = i + i
            while k <= size
                flags[k] = false
                k += i
            end
        end
    end
    return prime_count
end

benchmark() = sieve(fill(true, 5000), 5000)

function main()
    total = 0
    for _ in 1:ITERATIONS
        total += benchmark()
    end
    println(total)
end

main()
