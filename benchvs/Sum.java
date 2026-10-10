// Sum benchmark - sum integers from 1 to 30,000,000 into an arbitrary-precision int.

import java.math.BigInteger;

public class Sum {
    static BigInteger sumTo(int limit) {
        BigInteger total = BigInteger.ZERO;
        for (int i = 1; i <= limit; i++) {
            total = total.add(BigInteger.valueOf(i));
        }
        return total;
    }

    public static void main(String[] args) {
        System.out.println(sumTo(30_000_000));
    }
}
