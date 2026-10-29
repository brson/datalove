// Storage benchmark - build a tree of arrays seven levels deep, four children
// to a node, whose leaves are arrays of 1 to 10 empty slots, and count the
// nodes, many times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose tree has 5461 nodes each time.
// Prints the sum of the counts.

const ITERATIONS = 8000;

class Random {
  constructor() {
    this.seed = 74755;
  }

  next() {
    this.seed = ((this.seed * 1309) + 13849) & 65535;
    return this.seed;
  }
}

let count = 0;

function buildTreeDepth(depth, random) {
  count += 1;
  if (depth === 1) {
    return new Array((random.next() % 10) + 1);
  }
  const arr = new Array(4);
  for (let i = 0; i < 4; i += 1) {
    arr[i] = buildTreeDepth(depth - 1, random);
  }
  return arr;
}

function benchmark() {
  count = 0;
  buildTreeDepth(7, new Random());
  return count;
}

let total = 0;
for (let n = 0; n < ITERATIONS; n += 1) {
  total += benchmark();
}
print(total);
