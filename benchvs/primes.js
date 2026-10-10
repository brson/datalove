// Prime counting benchmark - count primes up to 4,000,000 by trial division.

function isPrime(n) {
  if (n < 2) {
    return false;
  }
  if (n === 2) {
    return true;
  }
  if (n % 2 === 0) {
    return false;
  }
  let d = 3;
  while (d * d <= n) {
    if (n % d === 0) {
      return false;
    }
    d += 2;
  }
  return true;
}

function countPrimes(limit) {
  let count = 0;
  for (let n = 2; n <= limit; n++) {
    if (isPrime(n)) {
      count++;
    }
  }
  return count;
}

print(countPrimes(4000000));
