// Sieve benchmark - count the primes up to 5000 with a sieve of Eratosthenes,
// many times over. From Are We Fast Yet (github.com/smarr/are-we-fast-yet),
// whose Sieve counts 669 each time. Prints the sum of the counts.

import java.util.Arrays;

public class Sieve {
    static final int ITERATIONS = 10000;

    static int sieve(final boolean[] flags, final int size) {
        int primeCount = 0;
        for (int i = 2; i <= size; i++) {
            if (flags[i - 1]) {
                primeCount++;
                int k = i + i;
                while (k <= size) {
                    flags[k - 1] = false;
                    k += i;
                }
            }
        }
        return primeCount;
    }

    static int benchmark() {
        boolean[] flags = new boolean[5000];
        Arrays.fill(flags, true);
        return sieve(flags, 5000);
    }

    public static void main(String[] args) {
        int total = 0;
        for (int n = 0; n < ITERATIONS; n++) {
            total += benchmark();
        }
        System.out.println(total);
    }
}
