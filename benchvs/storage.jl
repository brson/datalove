#!/usr/bin/env julia
# Storage benchmark - build a tree of arrays seven levels deep, four children
# to a node, whose leaves are arrays of 1 to 10 empty slots, and count the
# nodes, many times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose tree has 5461 nodes each time.
# Prints the sum of the counts.

const ITERATIONS = 8000

mutable struct Random
    seed::Int
end

function next!(r::Random)
    r.seed = ((r.seed * 1309) + 13849) & 65535
    return r.seed
end

function build_tree_depth(depth::Int, random::Random, count::Ref{Int})::Vector{Any}
    count[] += 1
    if depth == 1
        return Vector{Any}(nothing, next!(random) % 10 + 1)
    end
    arr = Vector{Any}(nothing, 4)
    for i in 1:4
        arr[i] = build_tree_depth(depth - 1, random, count)
    end
    return arr
end

function benchmark()
    count = Ref(0)
    build_tree_depth(7, Random(74755), count)
    return count[]
end

function main()
    total = 0
    for _ in 1:ITERATIONS
        total += benchmark()
    end
    println(total)
end

main()
