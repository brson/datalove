// Ordered benchmark - generic ordered-collection algorithms at four element
// types: a binary heap used as a heapsort, a check that the result is sorted,
// a lower-bound binary search for a stream of probes, and a count of distinct
// elements. See ordered.dfs. Generic the way JavaScript is: each algorithm
// takes the element type's comparison as a function. Prints the sum of the
// per-type checksums.

const ITERATIONS = 200;
const SIZE = 1000;

let seed = 74755;

// AWFY's Random: a 16-bit linear congruential generator.
function next() {
  seed = ((seed * 1309) + 13849) & 65535;
  return seed;
}

function compareScalar(a, b) {
  return a < b ? -1 : (a > b ? 1 : 0);
}

function comparePair(a, b) {
  const c = compareScalar(a.word, b.word);
  return c !== 0 ? c : compareScalar(a.n, b.n);
}

function lessAt(h, i, j, compare) {
  return compare(h[i], h[j]) < 0;
}

function heapPush(h, elem, compare) {
  h.push(elem);
  let i = h.length - 1;
  while (i > 0) {
    const parent = (i - 1) >> 1;
    if (!lessAt(h, i, parent, compare)) {
      break;
    }
    const t = h[i];
    h[i] = h[parent];
    h[parent] = t;
    i = parent;
  }
}

function heapPop(h, compare) {
  const n = h.length;
  if (n === 0) {
    return undefined;
  }
  const last = n - 1;
  const t = h[0];
  h[0] = h[last];
  h[last] = t;
  const least = h.pop();
  let i = 0;
  for (;;) {
    const left = i * 2 + 1;
    if (left >= last) {
      break;
    }
    let child = left;
    const right = left + 1;
    if (right < last && lessAt(h, right, left, compare)) {
      child = right;
    }
    if (!lessAt(h, child, i, compare)) {
      break;
    }
    const u = h[i];
    h[i] = h[child];
    h[child] = u;
    i = child;
  }
  return least;
}

function heapsort(elems, compare) {
  const heap = [];
  while (elems.length > 0) {
    heapPush(heap, elems.pop(), compare);
  }
  const out = [];
  let elem;
  while ((elem = heapPop(heap, compare)) !== undefined) {
    out.push(elem);
  }
  return out;
}

function outOfOrder(xs, compare) {
  let count = 0;
  for (let i = 1; i < xs.length; i += 1) {
    if (lessAt(xs, i, i - 1, compare)) {
      count += 1;
    }
  }
  return count;
}

function lowerBound(xs, elem, compare) {
  let low = 0;
  let high = xs.length;
  while (low < high) {
    const mid = low + ((high - low) >> 1);
    if (compare(xs[mid], elem) < 0) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  return low;
}

function searchAll(xs, probes, compare) {
  let total = 0;
  const n = xs.length;
  for (const probe of probes) {
    const at = lowerBound(xs, probe, compare);
    total += at;
    if (at < n && compare(xs[at], probe) === 0) {
      total += 1;
    }
  }
  return total;
}

function distinct(xs, compare) {
  if (xs.length === 0) {
    return 0;
  }
  let count = 1;
  for (let i = 1; i < xs.length; i += 1) {
    if (lessAt(xs, i - 1, i, compare)) {
      count += 1;
    }
  }
  return count;
}

function checksum(elems, probes, compare) {
  const sorted = heapsort(elems, compare);
  return outOfOrder(sorted, compare) + searchAll(sorted, probes, compare) + distinct(sorted, compare);
}

// Three letters from 'a' to 'h', one for each three of the low nine bits.
function word(r) {
  return String.fromCharCode(97 + (r & 7), 97 + ((r >> 3) & 7), 97 + ((r >> 6) & 7));
}

function ints() {
  const xs = [];
  for (let i = 0; i < SIZE; i += 1) {
    xs.push(next() % 500);
  }
  return xs;
}

function floats() {
  const xs = [];
  for (let i = 0; i < SIZE; i += 1) {
    xs.push((next() % 500) * 0.25);
  }
  return xs;
}

function words() {
  const xs = [];
  for (let i = 0; i < SIZE; i += 1) {
    xs.push(word(next()));
  }
  return xs;
}

function pairs() {
  const xs = [];
  for (let i = 0; i < SIZE; i += 1) {
    const r = next();
    xs.push({ word: word(r >> 2), n: r & 3 });
  }
  return xs;
}

function benchmark() {
  let total = 0;
  const intProbes = ints();
  total += checksum(ints(), intProbes, compareScalar);
  const floatProbes = floats();
  total += checksum(floats(), floatProbes, compareScalar);
  const wordProbes = words();
  total += checksum(words(), wordProbes, compareScalar);
  const pairProbes = pairs();
  total += checksum(pairs(), pairProbes, comparePair);
  return total;
}

let total = 0;
for (let n = 0; n < ITERATIONS; n += 1) {
  total = (total + benchmark()) >>> 0;
}
print(total);
