// Fibonacci benchmark - compute fib(38) by naive recursion.

public class Fib {
    static long fib(long n) {
        if (n < 2) {
            return n;
        }
        return fib(n - 1) + fib(n - 2);
    }

    public static void main(String[] args) {
        System.out.println(fib(38));
    }
}
