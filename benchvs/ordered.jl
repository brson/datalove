#!/usr/bin/env julia
# Ordered benchmark - generic ordered-collection algorithms at four element
# types: a binary heap used as a heapsort, a check that the result is sorted,
# a lower-bound binary search for a stream of probes, and a count of distinct
# elements. See ordered.dfs. Generic over `isless`, which Julia compiles once
# per element type; a pair is a tuple. Prints the sum of the per-type
# checksums.

const ITERATIONS = 200
const SIZE = 1000

# AWFY's Random: a 16-bit linear congruential generator.
mutable struct Random
    seed::Int
end

function next!(r::Random)::Int
    r.seed = ((r.seed * 1309) + 13849) & 65535
    return r.seed
end

function heap_push!(h::Vector{T}, elem::T) where {T}
    push!(h, elem)
    i = length(h)
    while i > 1
        parent = (i - 2) >> 1 + 1
        if !isless(h[i], h[parent])
            break
        end
        h[i], h[parent] = h[parent], h[i]
        i = parent
    end
end

function heap_pop!(h::Vector{T})::Union{T, Nothing} where {T}
    n = length(h)
    if n == 0
        return nothing
    end
    h[1], h[n] = h[n], h[1]
    least = pop!(h)
    last = n - 1
    i = 1
    while true
        left = 2 * i
        if left > last
            break
        end
        child = left
        right = left + 1
        if right <= last && isless(h[right], h[left])
            child = right
        end
        if !isless(h[child], h[i])
            break
        end
        h[i], h[child] = h[child], h[i]
        i = child
    end
    return least
end

function heapsort!(elems::Vector{T})::Vector{T} where {T}
    heap = T[]
    while !isempty(elems)
        heap_push!(heap, pop!(elems))
    end
    out = T[]
    while true
        elem = heap_pop!(heap)
        elem === nothing && break
        push!(out, elem)
    end
    return out
end

function out_of_order(xs::Vector{T})::Int where {T}
    count = 0
    for i in 2:length(xs)
        if isless(xs[i], xs[i - 1])
            count += 1
        end
    end
    return count
end

# The first 0-based position whose element does not sort before `elem`.
function lower_bound(xs::Vector{T}, elem::T)::Int where {T}
    low = 0
    high = length(xs)
    while low < high
        mid = low + ((high - low) >> 1)
        if isless(xs[mid + 1], elem)
            low = mid + 1
        else
            high = mid
        end
    end
    return low
end

function search_all(xs::Vector{T}, probes::Vector{T})::Int where {T}
    total = 0
    n = length(xs)
    for probe in probes
        at = lower_bound(xs, probe)
        total += at
        if at < n && isequal(xs[at + 1], probe)
            total += 1
        end
    end
    return total
end

function distinct(xs::Vector{T})::Int where {T}
    isempty(xs) && return 0
    count = 1
    for i in 2:length(xs)
        if isless(xs[i - 1], xs[i])
            count += 1
        end
    end
    return count
end

function checksum(elems::Vector{T}, probes::Vector{T})::Int where {T}
    sorted = heapsort!(elems)
    return out_of_order(sorted) + search_all(sorted, probes) + distinct(sorted)
end

# Three letters from 'a' to 'h', one for each three of the low nine bits.
word(r::Int)::String = String([Char(97 + (r & 7)), Char(97 + ((r >> 3) & 7)), Char(97 + ((r >> 6) & 7))])

ints(rng::Random) = [next!(rng) % 500 for _ in 1:SIZE]
floats(rng::Random) = [(next!(rng) % 500) * 0.25 for _ in 1:SIZE]
words(rng::Random) = [word(next!(rng)) for _ in 1:SIZE]

function pairs(rng::Random)::Vector{Tuple{String, Int}}
    xs = Tuple{String, Int}[]
    for _ in 1:SIZE
        r = next!(rng)
        push!(xs, (word(r >> 2), r & 3))
    end
    return xs
end

function benchmark(rng::Random)::Int
    total = 0
    int_probes = ints(rng)
    total += checksum(ints(rng), int_probes)
    float_probes = floats(rng)
    total += checksum(floats(rng), float_probes)
    word_probes = words(rng)
    total += checksum(words(rng), word_probes)
    pair_probes = pairs(rng)
    total += checksum(pairs(rng), pair_probes)
    return total
end

function main()
    rng = Random(74755)
    total = 0
    for _ in 1:ITERATIONS
        total = (total + benchmark(rng)) & 0xFFFFFFFF
    end
    println(total)
end

main()
