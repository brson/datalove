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
    'type-name': /\b(?:bool|u8|u16|u32|u64|i8|i16|i32|i64|index|offset|int|f32|f64|string|list|map|set|tensor|table|data|error)\b/,
    'keyword': /\b(?:let|var|const|set|type|fun|ret|if|else|end|require|import|break|continue|loop|while|module|in|out|mut|ref|and|or|xor|not|icall|debuglog|data)\b/,
    'function': /\b[a-z_][a-z0-9_]*(?=\s*\()/,
    'op-cmp': /\.\<|\.\>|<=|>=|==|!=/,
    'op-result': /[+\-*\/]!|!/,
    'op-option': /[+\-*\/]\?|\?/,
    'op-clone': /@/,
    'op-arith': /[+\-*\/]/,
    'brace-table': /\{\||\|\}/,
    'brace-curly': /[{}]/,
    'brace-paren': /[()]/,
    'brace-angle': /[<>]/,
    'brace-square': /[\[\]]/,
    'type-hint-slash': /\//,
    'type-colon': /:/,
    'punctuation': /[;,]/
};
Prism.languages.datalit = Prism.languages.datalove;
Prism.languages.datafun = Prism.languages.datalove;
