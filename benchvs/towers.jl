#!/usr/bin/env julia
# Towers benchmark - the towers of Hanoi with each pile a linked list of
# disks, moving 13 disks many times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose Towers makes 8191 moves each
# time. Prints the sum of the moves.

const ITERATIONS = 9000

mutable struct Disk
    size::Int
    next::Union{Disk, Nothing}
end

mutable struct Towers
    piles::Vector{Union{Disk, Nothing}}
    moves_done::Int
end

function push_disk!(t::Towers, disk::Disk, pile::Int)
    top = t.piles[pile]
    if top !== nothing && disk.size >= top.size
        error("Cannot put a big disk on a smaller one")
    end
    disk.next = top
    t.piles[pile] = disk
end

function pop_disk_from!(t::Towers, pile::Int)::Disk
    top = t.piles[pile]
    if top === nothing
        error("Attempting to remove a disk from an empty pile")
    end
    t.piles[pile] = top.next
    top.next = nothing
    return top
end

function move_top_disk!(t::Towers, from::Int, to::Int)
    push_disk!(t, pop_disk_from!(t, from), to)
    t.moves_done += 1
end

function move_disks!(t::Towers, disks::Int, from::Int, to::Int)
    if disks == 1
        move_top_disk!(t, from, to)
    else
        other = (6 - from) - to
        move_disks!(t, disks - 1, from, other)
        move_top_disk!(t, from, to)
        move_disks!(t, disks - 1, other, to)
    end
end

function benchmark()
    t = Towers(Union{Disk, Nothing}[nothing, nothing, nothing], 0)
    for i in 13:-1:0
        push_disk!(t, Disk(i, nothing), 1)
    end
    t.moves_done = 0
    move_disks!(t, 13, 1, 2)
    return t.moves_done
end

function main()
    total = 0
    for _ in 1:ITERATIONS
        total += benchmark()
    end
    println(total)
end

main()
