// Word frequency benchmark - count the words of a generated text and report the
// most frequent. See wordfreq.dfs for the shape of the text, which every
// implementation generates identically from the same seed.
//
// Prints (words, distinct words, checksum of the top 20 by count).

const WORDS = 2000000;
const TOP = 20;

function main() {
  const syllables = ["ka", "lo", "mi", "ra", "te", "su", "no", "vi",
                     "de", "pa", "zu", "ri", "go", "ne", "ba", "to"];
  const vocab = [];
  for (let i = 0; i < 4096; i++) {
    vocab.push(syllables[i & 15] + syllables[(i >> 4) & 15] + syllables[i >> 8]);
  }

  // The generator is inlined; a call per draw would mostly measure calls.
  // Arithmetic is on 32-bit unsigned integers, as in the other implementations.
  let state = 12345;
  const parts = [];
  for (let i = 0; i < WORDS; i++) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    const a = (state >>> 16) % 4096;
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    const b = (state >>> 16) % 4096;
    let word = vocab[Math.min(a, b)];
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    if ((state >>> 16) % 10 === 0) {
      word = word.toUpperCase();
    }
    parts.push(word);
    parts.push(i % 12 === 11 ? "\n" : " ");
  }
  const text = parts.join("");

  const counts = new Map();
  for (const word of text.split(/\s+/)) {
    if (word === "") {
      continue;
    }
    const lower = word.toLowerCase();
    counts.set(lower, (counts.get(lower) || 0) + 1);
  }

  // By count descending, then by word.
  const ranked = [...counts.entries()].sort((x, y) =>
    y[1] - x[1] || (x[0] < y[0] ? -1 : x[0] > y[0] ? 1 : 0));
  let checksum = 0;
  for (const [word, count] of ranked.slice(0, TOP)) {
    checksum = (Math.imul(checksum, 31) + count) >>> 0;
    // The words are ASCII, so their UTF-16 code units are their bytes.
    for (let k = 0; k < word.length; k++) {
      checksum = (Math.imul(checksum, 31) + word.charCodeAt(k)) >>> 0;
    }
  }

  print(`(${WORDS}, ${counts.size}, ${checksum})`);
}

main();
