#!/usr/bin/env python3
# Towers benchmark - the towers of Hanoi with each pile a linked list of
# disks, moving 13 disks many times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose Towers makes 8191 moves each
# time. Prints the sum of the moves.

ITERATIONS = 9000


class Disk:
    __slots__ = ("size", "next")

    def __init__(self, size):
        self.size = size
        self.next = None


class Towers:
    def __init__(self):
        self.piles = None
        self.moves_done = 0

    def push_disk(self, disk, pile):
        top = self.piles[pile]
        if top is not None and disk.size >= top.size:
            raise Exception("Cannot put a big disk on a smaller one")
        disk.next = top
        self.piles[pile] = disk

    def pop_disk_from(self, pile):
        top = self.piles[pile]
        if top is None:
            raise Exception("Attempting to remove a disk from an empty pile")
        self.piles[pile] = top.next
        top.next = None
        return top

    def move_top_disk(self, from_pile, to_pile):
        self.push_disk(self.pop_disk_from(from_pile), to_pile)
        self.moves_done += 1

    def build_tower_at(self, pile, disks):
        for i in range(disks, -1, -1):
            self.push_disk(Disk(i), pile)

    def move_disks(self, disks, from_pile, to_pile):
        if disks == 1:
            self.move_top_disk(from_pile, to_pile)
        else:
            other_pile = (3 - from_pile) - to_pile
            self.move_disks(disks - 1, from_pile, other_pile)
            self.move_top_disk(from_pile, to_pile)
            self.move_disks(disks - 1, other_pile, to_pile)

    def benchmark(self):
        self.piles = [None, None, None]
        self.build_tower_at(0, 13)
        self.moves_done = 0
        self.move_disks(13, 0, 1)
        return self.moves_done


towers = Towers()
total = 0
for _ in range(ITERATIONS):
    total += towers.benchmark()
print(total)
