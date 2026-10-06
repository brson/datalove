// Prism.js language definition for Datalove.
// Keep in step with datalove-mode.el.
Prism.languages.datalove = {
    'comment': [
        /\/\/.*/,
        /\/\*[\s\S]*?\*\//
    ],
    'string': /"(?:[^"\\]|\\.)*"/,
    'number': /\b0x[0-9a-fA-F]+\b|\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/,
    // The path in `require module lib/pkg/module`, so its slashes are not
    // read as division.
    'module-path': {
        pattern: /(\brequire\s+module\s+)[A-Za-z_]\w*\/[A-Za-z_]\w*\/[A-Za-z_]\w*/,
        lookbehind: true
    },
    'boolean':/\b(?:true|false)\b/,
    'variant': /\b(?:ok|er|some|none)\b/,
    // The bound a type parameter is given: `with { T is ord, }`.
    'bound-name': {
        pattern: /(\bis\s+)(?:float|fixedint|ord)\b/,
        lookbehind: true,
        alias: 'type-name'
    },
    'type-name': /\b(?:bool|u8|u16|u32|u64|i8|i16|i32|i64|index|offset|int|f32|f64|string|data|error|tuple|atom|term|enum)\b/,
    'keyword': /\b(?:let|var|const|set|call|ret|require|module|import|rider|with|is|and|or|xor|not|icall|debuglog)\b/,
    'binding-modifier': /\b(?:ref|mut|out)\b/,
    'item-keyword': /\b(?:type|native|fun|end\s+fun)\b/,
    'control-flow': /\b(?:if|else|end\s+if|loop|while|break|continue|end\s+loop|match|case|default|end\s+match)\b/,
    'function': /\b[a-z_][a-z0-9_]*(?=\s*\()/,
    'op-cmp': /\.<|\.>|<=|>=|==|!=/,
    'op-result': /[+\-*\/]!|!/,
    'op-option': /[+\-*\/]\?|\?/,
    'op-clone': /@/,
    'op-arith': /[+\-*\/]/,
    'brace-table': /\{\||\|\}/,
    'brace-tensor': /\[\||\|\]/,
    'brace-map': /%\{/,
    'brace-set': /#\{/,
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
