// Prism.js language definition for Datalove
Prism.languages.datalove = {
    'comment': [
        /\/\/.*/,
        /\/\*[\s\S]*?\*\//
    ],
    'string': /"(?:[^"\\]|\\.)*"/,
    'number': /\b0x[0-9a-fA-F]+\b|\b\d+(?:\.\d+)?\b/,
    'boolean': /\b(?:true|false)\b/,
    'variant': /\b(?:ok|er|some|none)\b/,
    'type-name': /\b(?:bool|u8|u16|u32|u64|i8|i16|i32|i64|index|offset|int|f32|f64|string|list|tensor|table|data|error|tuple|atom|term|enum)\b/,
    'keyword': /\b(?:ret|let|var|set|require|import|default|and|or|xor|not|icall|debuglog|data)\b/,
    'binding-modifier': /\b(?:ref|mut|out)\b/,
    'item-keyword': /\b(?:type|fun|end fun)\b/,
    'control-flow': /\b(?:if|else|end if|break|continue|loop|end loop|while|for|end for|match|end match|case)\b/,
    'function': /\b[a-z_][a-z0-9_]*(?=\s*\()/,
    'op-cmp': /[<>]|[\u2264\u2265\u2261\u2262]/,
    'op-result': /[+\-*\/]!|!/,
    'op-option': /[+\-*\/]\?|\?/,
    'op-clone': /@/,
    'op-arith': /[+\-*\/]/,
    'maps-to': /\u21A6/,
    'brace-table': /[\u27E6\u27E7]/,
    'brace-tensor': /[\u27EA\u27EB]/,
    'brace-map': /[\u2987\u2988]/,
    'brace-set': /[\u2983\u2984]/,
    'brace-curly': /[{}]/,
    'brace-paren': /[()]/,
    'brace-square': /[\[\]]/,
    'type-hint-slash': /\//,
    'type-colon': /:/,
    'punctuation': /[;,]/
};
Prism.languages.datalit = Prism.languages.datalove;
Prism.languages.datafun = Prism.languages.datalove;
