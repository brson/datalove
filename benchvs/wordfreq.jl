#!/usr/bin/env julia
# Word frequency benchmark - count the words of a generated text and report the
# most frequent. See wordfreq.dfs for the shape of the text, which every
# implementation generates identically from the same seed.
#
# Prints (words, distinct words, checksum of the top 20 by count).

const WORDS = 2_000_000
const TOP = 20

next(state::UInt32) = state * UInt32(1664525) + UInt32(1013904223)

function main()
    syllables = ["ka", "lo", "mi", "ra", "te", "su", "no", "vi",
                 "de", "pa", "zu", "ri", "go", "ne", "ba", "to"]
    vocab = [syllables[(i & 15) + 1] * syllables[((i >> 4) & 15) + 1] * syllables[(i >> 8) + 1]
             for i in 0:4095]

    state = UInt32(12345)
    io = IOBuffer()
    for i in 0:WORDS-1
        state = next(state)
        a = (state >> 16) % 4096
        state = next(state)
        b = (state >> 16) % 4096
        word = vocab[min(a, b) + 1]
        state = next(state)
        if (state >> 16) % 10 == 0
            word = uppercase(word)
        end
        print(io, word)
        print(io, i % 12 == 11 ? '\n' : ' ')
    end
    text = String(take!(io))

    counts = Dict{String,Int}()
    for word in split(text)
        lower = lowercase(word)
        counts[lower] = get(counts, lower, 0) + 1
    end

    # By count descending, then by word.
    ranked = sort!(collect(counts), by = kv -> (-kv[2], kv[1]))
    checksum = UInt32(0)
    for (word, count) in ranked[1:TOP]
        checksum = checksum * UInt32(31) + UInt32(count)
        for byte in codeunits(word)
            checksum = checksum * UInt32(31) + byte
        end
    end

    println("($WORDS, $(length(counts)), $checksum)")
end

main()
