// Mandelbrot benchmark - the Mandelbrot set at 750 by 750, folded into a byte
// checksum, several times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose Mandelbrot at 750 gives 50, and
// before it the Computer Language Benchmarks Game. Prints the sum of the
// checksums.

public class Mandelbrot {
    static final int ITERATIONS = 4;
    static final int SIZE = 750;

    static int mandelbrot(final int size) {
        int sum = 0;
        int byteAcc = 0;
        int bitNum = 0;
        int y = 0;
        while (y < size) {
            double ci = (2.0 * y / size) - 1.0;
            int x = 0;
            while (x < size) {
                double zrzr = 0.0;
                double zi = 0.0;
                double zizi = 0.0;
                double cr = (2.0 * x / size) - 1.5;
                int z = 0;
                boolean notDone = true;
                int escape = 0;
                while (notDone && z < 50) {
                    double zr = zrzr - zizi + cr;
                    zi = 2.0 * zr * zi + ci;
                    zrzr = zr * zr;
                    zizi = zi * zi;
                    if (zrzr + zizi > 4.0) {
                        notDone = false;
                        escape = 1;
                    }
                    z += 1;
                }
                byteAcc = (byteAcc << 1) + escape;
                bitNum += 1;
                if (bitNum == 8) {
                    sum ^= byteAcc;
                    byteAcc = 0;
                    bitNum = 0;
                } else if (x == size - 1) {
                    byteAcc <<= (8 - bitNum);
                    sum ^= byteAcc;
                    byteAcc = 0;
                    bitNum = 0;
                }
                x += 1;
            }
            y += 1;
        }
        return sum;
    }

    public static void main(String[] args) {
        int total = 0;
        for (int n = 0; n < ITERATIONS; n++) {
            total += mandelbrot(SIZE);
        }
        System.out.println(total);
    }
}
