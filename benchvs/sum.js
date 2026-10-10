// Sum benchmark - sum integers from 1 to 30,000,000 into an arbitrary-precision int.

function sumTo(limit) {
  let total = 0n;
  for (let i = 1; i <= limit; i++) {
    total += BigInt(i);
  }
  return total;
}

print(sumTo(30000000).toString());
