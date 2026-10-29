// Towers benchmark - the towers of Hanoi with each pile a linked list of
// disks, moving 13 disks many times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose Towers makes 8191 moves each
// time. Prints the sum of the moves.

const ITERATIONS = 9000;

class TowersDisk {
  constructor(size) {
    this.size = size;
    this.next = null;
  }
}

class Towers {
  constructor() {
    this.piles = null;
    this.movesDone = 0;
  }

  pushDisk(disk, pile) {
    const top = this.piles[pile];
    if (top && disk.size >= top.size) {
      throw new Error('Cannot put a big disk on a smaller one');
    }
    disk.next = top;
    this.piles[pile] = disk;
  }

  popDiskFrom(pile) {
    const top = this.piles[pile];
    if (top === null) {
      throw new Error('Attempting to remove a disk from an empty pile');
    }
    this.piles[pile] = top.next;
    top.next = null;
    return top;
  }

  moveTopDisk(fromPile, toPile) {
    this.pushDisk(this.popDiskFrom(fromPile), toPile);
    this.movesDone += 1;
  }

  buildTowerAt(pile, disks) {
    for (let i = disks; i >= 0; i -= 1) {
      this.pushDisk(new TowersDisk(i), pile);
    }
  }

  moveDisks(disks, fromPile, toPile) {
    if (disks === 1) {
      this.moveTopDisk(fromPile, toPile);
    } else {
      const otherPile = (3 - fromPile) - toPile;
      this.moveDisks(disks - 1, fromPile, otherPile);
      this.moveTopDisk(fromPile, toPile);
      this.moveDisks(disks - 1, otherPile, toPile);
    }
  }

  benchmark() {
    this.piles = [null, null, null];
    this.buildTowerAt(0, 13);
    this.movesDone = 0;
    this.moveDisks(13, 0, 1);
    return this.movesDone;
  }
}

const towers = new Towers();
let total = 0;
for (let n = 0; n < ITERATIONS; n += 1) {
  total += towers.benchmark();
}
print(total);
