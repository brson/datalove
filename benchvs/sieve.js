// Sieve benchmark - count the primes up to 5000 with a sieve of Eratosthenes,
// many times over. From Are We Fast Yet (github.com/smarr/are-we-fast-yet),
// whose Sieve counts 669 each time. Prints the sum of the counts.

const ITERATIONS = 10000;

function sieve(flags, size) {
  let primeCount = 0;
  for (let i = 2; i <= size; i += 1) {
    if (flags[i - 1]) {
      primeCount += 1;
      let k = i + i;
      while (k <= size) {
        flags[k - 1] = false;
        k += i;
      }
    }
  }
  return primeCount;
}

function benchmark() {
  const flags = new Array(5000);
  flags.fill(true);
  return sieve(flags, 5000);
}

let total = 0;
for (let n = 0; n < ITERATIONS; n += 1) {
  total += benchmark();
}
print(total);
