// Storage benchmark - build a tree of arrays seven levels deep, four children
// to a node, whose leaves are arrays of 1 to 10 empty slots, and count the
// nodes, many times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose tree has 5461 nodes each time.
// Prints the sum of the counts.

public class Storage {
    static final int ITERATIONS = 8000;

    static final class Random {
        private int seed = 74755;

        int next() {
            seed = ((seed * 1309) + 13849) & 65535;
            return seed;
        }
    }

    private int count;

    Object buildTreeDepth(final int depth, final Random random) {
        count++;
        if (depth == 1) {
            return new Object[random.next() % 10 + 1];
        }
        Object[] arr = new Object[4];
        for (int i = 0; i < 4; i++) {
            arr[i] = buildTreeDepth(depth - 1, random);
        }
        return arr;
    }

    int benchmark() {
        count = 0;
        buildTreeDepth(7, new Random());
        return count;
    }

    public static void main(String[] args) {
        Storage storage = new Storage();
        int total = 0;
        for (int n = 0; n < ITERATIONS; n++) {
            total += storage.benchmark();
        }
        System.out.println(total);
    }
}
