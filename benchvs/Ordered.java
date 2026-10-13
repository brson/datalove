// Ordered benchmark - generic ordered-collection algorithms at four element
// types: a binary heap used as a heapsort, a check that the result is sorted,
// a lower-bound binary search for a stream of probes, and a count of distinct
// elements. See ordered.dfs. Generic over Comparable, so the int and double
// elements are boxed, as they are in any Java generic code. Prints the sum of
// the per-type checksums.

import java.util.ArrayList;

public class Ordered {
    static final int ITERATIONS = 200;
    static final int SIZE = 1000;

    static int seed = 74755;

    // AWFY's Random: a 16-bit linear congruential generator.
    static int next() {
        seed = ((seed * 1309) + 13849) & 65535;
        return seed;
    }

    static final class Pair implements Comparable<Pair> {
        final String word;
        final int n;

        Pair(final String word, final int n) {
            this.word = word;
            this.n = n;
        }

        @Override
        public int compareTo(final Pair other) {
            final int c = word.compareTo(other.word);
            return c != 0 ? c : Integer.compare(n, other.n);
        }
    }

    static <T extends Comparable<T>> boolean lessAt(final ArrayList<T> h, final int i, final int j) {
        return h.get(i).compareTo(h.get(j)) < 0;
    }

    static <T extends Comparable<T>> void heapPush(final ArrayList<T> h, final T elem) {
        h.add(elem);
        int i = h.size() - 1;
        while (i > 0) {
            final int parent = (i - 1) >> 1;
            if (!lessAt(h, i, parent)) {
                break;
            }
            final T t = h.get(i);
            h.set(i, h.get(parent));
            h.set(parent, t);
            i = parent;
        }
    }

    static <T extends Comparable<T>> T heapPop(final ArrayList<T> h) {
        final int n = h.size();
        if (n == 0) {
            return null;
        }
        final int last = n - 1;
        final T t = h.get(0);
        h.set(0, h.get(last));
        h.set(last, t);
        final T least = h.remove(last);
        int i = 0;
        while (true) {
            final int left = i * 2 + 1;
            if (left >= last) {
                break;
            }
            int child = left;
            final int right = left + 1;
            if (right < last && lessAt(h, right, left)) {
                child = right;
            }
            if (!lessAt(h, child, i)) {
                break;
            }
            final T u = h.get(i);
            h.set(i, h.get(child));
            h.set(child, u);
            i = child;
        }
        return least;
    }

    static <T extends Comparable<T>> ArrayList<T> heapsort(final ArrayList<T> elems) {
        final ArrayList<T> heap = new ArrayList<>();
        while (!elems.isEmpty()) {
            heapPush(heap, elems.remove(elems.size() - 1));
        }
        final ArrayList<T> out = new ArrayList<>();
        T elem;
        while ((elem = heapPop(heap)) != null) {
            out.add(elem);
        }
        return out;
    }

    static <T extends Comparable<T>> int outOfOrder(final ArrayList<T> xs) {
        int count = 0;
        for (int i = 1; i < xs.size(); i++) {
            if (lessAt(xs, i, i - 1)) {
                count++;
            }
        }
        return count;
    }

    static <T extends Comparable<T>> int lowerBound(final ArrayList<T> xs, final T elem) {
        int low = 0;
        int high = xs.size();
        while (low < high) {
            final int mid = low + ((high - low) >> 1);
            if (xs.get(mid).compareTo(elem) < 0) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        return low;
    }

    static <T extends Comparable<T>> int searchAll(final ArrayList<T> xs, final ArrayList<T> probes) {
        int total = 0;
        final int n = xs.size();
        for (final T probe : probes) {
            final int at = lowerBound(xs, probe);
            total += at;
            if (at < n && xs.get(at).compareTo(probe) == 0) {
                total += 1;
            }
        }
        return total;
    }

    static <T extends Comparable<T>> int distinct(final ArrayList<T> xs) {
        if (xs.isEmpty()) {
            return 0;
        }
        int count = 1;
        for (int i = 1; i < xs.size(); i++) {
            if (lessAt(xs, i - 1, i)) {
                count++;
            }
        }
        return count;
    }

    static <T extends Comparable<T>> int checksum(final ArrayList<T> elems, final ArrayList<T> probes) {
        final ArrayList<T> sorted = heapsort(elems);
        return outOfOrder(sorted) + searchAll(sorted, probes) + distinct(sorted);
    }

    // Three letters from 'a' to 'h', one for each three of the low nine bits.
    static String word(final int r) {
        final char[] s = {
            (char) ('a' + (r & 7)),
            (char) ('a' + ((r >> 3) & 7)),
            (char) ('a' + ((r >> 6) & 7)),
        };
        return new String(s);
    }

    static ArrayList<Integer> ints() {
        final ArrayList<Integer> xs = new ArrayList<>();
        for (int i = 0; i < SIZE; i++) {
            xs.add(next() % 500);
        }
        return xs;
    }

    static ArrayList<Double> floats() {
        final ArrayList<Double> xs = new ArrayList<>();
        for (int i = 0; i < SIZE; i++) {
            xs.add((next() % 500) * 0.25);
        }
        return xs;
    }

    static ArrayList<String> words() {
        final ArrayList<String> xs = new ArrayList<>();
        for (int i = 0; i < SIZE; i++) {
            xs.add(word(next()));
        }
        return xs;
    }

    static ArrayList<Pair> pairs() {
        final ArrayList<Pair> xs = new ArrayList<>();
        for (int i = 0; i < SIZE; i++) {
            final int r = next();
            xs.add(new Pair(word(r >> 2), r & 3));
        }
        return xs;
    }

    static long benchmark() {
        long total = 0;
        final ArrayList<Integer> intProbes = ints();
        total += checksum(ints(), intProbes);
        final ArrayList<Double> floatProbes = floats();
        total += checksum(floats(), floatProbes);
        final ArrayList<String> wordProbes = words();
        total += checksum(words(), wordProbes);
        final ArrayList<Pair> pairProbes = pairs();
        total += checksum(pairs(), pairProbes);
        return total;
    }

    public static void main(final String[] args) {
        long total = 0;
        for (int n = 0; n < ITERATIONS; n++) {
            total = (total + benchmark()) & 0xFFFFFFFFL;
        }
        System.out.println(total);
    }
}
