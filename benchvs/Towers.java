// Towers benchmark - the towers of Hanoi with each pile a linked list of
// disks, moving 13 disks many times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose Towers makes 8191 moves each
// time. Prints the sum of the moves.

public class Towers {
    static final int ITERATIONS = 9000;

    static final class Disk {
        final int size;
        Disk next;

        Disk(final int size) {
            this.size = size;
        }
    }

    private Disk[] piles;
    private int movesDone;

    private void pushDisk(final Disk disk, final int pile) {
        Disk top = piles[pile];
        if (top != null && disk.size >= top.size) {
            throw new RuntimeException("Cannot put a big disk on a smaller one");
        }
        disk.next = top;
        piles[pile] = disk;
    }

    private Disk popDiskFrom(final int pile) {
        Disk top = piles[pile];
        if (top == null) {
            throw new RuntimeException("Attempting to remove a disk from an empty pile");
        }
        piles[pile] = top.next;
        top.next = null;
        return top;
    }

    private void moveTopDisk(final int fromPile, final int toPile) {
        pushDisk(popDiskFrom(fromPile), toPile);
        movesDone++;
    }

    private void buildTowerAt(final int pile, final int disks) {
        for (int i = disks; i >= 0; i--) {
            pushDisk(new Disk(i), pile);
        }
    }

    private void moveDisks(final int disks, final int fromPile, final int toPile) {
        if (disks == 1) {
            moveTopDisk(fromPile, toPile);
        } else {
            int otherPile = (3 - fromPile) - toPile;
            moveDisks(disks - 1, fromPile, otherPile);
            moveTopDisk(fromPile, toPile);
            moveDisks(disks - 1, otherPile, toPile);
        }
    }

    int benchmark() {
        piles = new Disk[3];
        buildTowerAt(0, 13);
        movesDone = 0;
        moveDisks(13, 0, 1);
        return movesDone;
    }

    public static void main(String[] args) {
        Towers towers = new Towers();
        int total = 0;
        for (int n = 0; n < ITERATIONS; n++) {
            total += towers.benchmark();
        }
        System.out.println(total);
    }
}
