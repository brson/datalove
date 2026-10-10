// Prime counting benchmark - count primes up to 4,000,000 by trial division.

public class Primes {
    static boolean isPrime(int n) {
        if (n < 2) {
            return false;
        }
        if (n == 2) {
            return true;
        }
        if (n % 2 == 0) {
            return false;
        }
        int d = 3;
        while (d * d <= n) {
            if (n % d == 0) {
                return false;
            }
            d += 2;
        }
        return true;
    }

    static int countPrimes(int limit) {
        int count = 0;
        for (int n = 2; n <= limit; n++) {
            if (isPrime(n)) {
                count++;
            }
        }
        return count;
    }

    public static void main(String[] args) {
        System.out.println(countPrimes(4_000_000));
    }
}
