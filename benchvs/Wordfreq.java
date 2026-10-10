// Word frequency benchmark - count the words of a generated text and report the
// most frequent. See wordfreq.dfs for the shape of the text, which every
// implementation generates identically from the same seed.
//
// Prints (words, distinct words, checksum of the top 20 by count).

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public class Wordfreq {
    static final int WORDS = 2_000_000;
    static final int TOP = 20;

    public static void main(String[] args) {
        String[] syllables = {"ka", "lo", "mi", "ra", "te", "su", "no", "vi",
                              "de", "pa", "zu", "ri", "go", "ne", "ba", "to"};
        String[] vocab = new String[4096];
        for (int i = 0; i < 4096; i++) {
            vocab[i] = syllables[i & 15] + syllables[(i >> 4) & 15] + syllables[i >> 8];
        }

        // The generator is inlined; a call per draw would mostly measure calls.
        // An int wraps as the others' 32-bit unsigned arithmetic does, and
        // `>>>` reads it unsigned.
        int state = 12345;
        StringBuilder text = new StringBuilder();
        for (int i = 0; i < WORDS; i++) {
            state = state * 1664525 + 1013904223;
            int a = (state >>> 16) % 4096;
            state = state * 1664525 + 1013904223;
            int b = (state >>> 16) % 4096;
            String word = vocab[Math.min(a, b)];
            state = state * 1664525 + 1013904223;
            if ((state >>> 16) % 10 == 0) {
                word = word.toUpperCase();
            }
            text.append(word);
            text.append(i % 12 == 11 ? '\n' : ' ');
        }

        Map<String, Integer> counts = new HashMap<>();
        for (String word : text.toString().split("\\s+")) {
            if (word.isEmpty()) {
                continue;
            }
            counts.merge(word.toLowerCase(), 1, Integer::sum);
        }

        // By count descending, then by word.
        List<Map.Entry<String, Integer>> ranked = new ArrayList<>(counts.entrySet());
        ranked.sort((x, y) -> {
            int byCount = Integer.compare(y.getValue(), x.getValue());
            return byCount != 0 ? byCount : x.getKey().compareTo(y.getKey());
        });
        int checksum = 0;
        for (Map.Entry<String, Integer> entry : ranked.subList(0, TOP)) {
            checksum = checksum * 31 + entry.getValue();
            for (byte c : entry.getKey().getBytes(StandardCharsets.UTF_8)) {
                checksum = checksum * 31 + (c & 0xFF);
            }
        }

        System.out.println("(" + WORDS + ", " + counts.size() + ", " + Integer.toUnsignedString(checksum) + ")");
    }
}
