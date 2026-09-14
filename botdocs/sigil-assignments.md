# Sigil Assignments

Current lexer sigils and their uses in the language.

Sigils are defined in the `bcts` crate (`lexer.rs`).

## Single-Character Sigils

### Punctuation
- `.` - Dot
  - Member access in import statements: `import u32.negate`
  - Part of comparison operators: `.<`, `.>`
  - Decimal point in float literals: `3.14`

- `,` - Comma
  - Separating items in lists, tuples, function parameters, etc.

- `;` - Semicolon
  - Statement separator (alternative to newline)

### Adapt / Clone / Widen
- `@` - At
  - Postfix adapt operator: clone, widen, or coerce

- `$` - Dollar
  - Lexed, not yet read by the parser: postfix clone, see
    [design-clone-and-coerce.md](design-clone-and-coerce.md)

- `~` - Tilde
  - Lexed, not yet read by the parser: postfix widen, see
    [design-clone-and-coerce.md](design-clone-and-coerce.md)

### Hash
- `#` - Hash
  - Part of `#{` set open sigil

### Percent
- `%` - Percent
  - Reserved, not yet used in parser. The `%{` map open is the longer match,
    so a `%` written against a `{` is still that.

### Type Constructors / Modifiers
- `?` - Question
  - Prefix: Option type constructor: `let x: ?u32`
  - Postfix: Try operator for Option (early return on None)

- `!` - Exclamation
  - Prefix: Result type constructor: `let x: !u32`
  - Postfix: Try operator for Result (early return on Error)

### Arithmetic Operators
- `+` - Plus
  - Total addition

- `-` - Minus
  - Total subtraction
  - Unary negation

- `*` - Star
  - Total multiplication

- `/` - SlashForward
  - Total division (panics on overflow/zero)
  - Path separator in require statements: `require module sys/std/u32`
  - Type hint / expression separator: `: u32 / @42`

### Structural
- `:` - Colon
  - Type hint separator: `let x: u32`
  - Function parameter types: `fun foo(x: u32)`
  - Function return types: `fun foo(): u32`
  - Struct field type separator: `struct { x: u32 }`

- `=` - Equals
  - Assignment in let statements: `let x = value`
  - Struct field assignment: `{ x = 1 }`
  - Map entry assignment: `%{ 1 = 10 }`
  - Map type key-value separator: `%{K = V}`
  - Carry binding: `loop carry (x = 0)`

- `|` - Pipe
  - Binding delimiter in if statements: `if condition |x|`

### Braces (Balanced Pairs)
- `(` `)` - ParenOpen, ParenClose
  - Function parameters: `fun foo(a: u32, b: u32)`
  - Function calls: `foo(1, 2)`
  - Tuples: `(1, 2, 3)`
  - Tuple type hints: `(u32, u32)`
  - Expression grouping
  - Carry/bring clauses: `loop carry (x = 0)`

- `{` `}` - BraceOpen, BraceClose
  - Structs: `{ x = 1, y = 2 }`
  - Enum type hints: `enum { atom A, term B u32 }`
  - Anonymous struct type hints: `{ x: u32 }`
  - Closes `%{` and `#{` sigils

- `[` `]` - BracketOpen, BracketClose
  - Lists: `[1, 2, 3]`
  - List types: `[u32]`

- `<` `>` - AngleOpen, AngleClose
  - Comparison operators: `.<`, `.>`

### Earmuff Braces (Pipe-Delimited)

- `[|` `|]` - BracketPipeOpen, BracketPipeClose
  - Tensor types: `[|u32, 2|]`
  - Tensor literals: `[| 1 2 3, 4 5 6 |]`
- `{|` `|}` - BracePipeOpen, BracePipeClose
  - Table types and literals: `{| x: u32, y: u32 |}`

### Asymmetric Braces (close with `}`)

- `%{` - PercentBraceOpen (closes with `}`)
  - Map types: `%{K = V}`
  - Map literals: `%{ 1 = 10, 2 = 20 }`
- `#{` - HashBraceOpen (closes with `}`)
  - Set types: `#{T}`
  - Set literals: `#{ 1, 2, 3 }`

## Two-Character Sigils

### Arithmetic Variants - Optional (?)
Return Option on overflow/error instead of panicking.

- `+?` - PlusQuestion: Optional addition
- `-?` - MinusQuestion: Optional subtraction / unary optional negation
- `*?` - StarQuestion: Optional multiplication
- `/?` - SlashQuestion: Optional division

### Arithmetic Variants - Checked (!)
Return Result on overflow/error instead of panicking.

- `+!` - PlusExclamation: Checked addition
- `-!` - MinusExclamation: Checked subtraction / unary checked negation
- `*!` - StarExclamation: Checked multiplication
- `/!` - SlashExclamation: Checked division

### Arithmetic Variants - Bar (|)
Reserved, not yet used in parser.

- `+|` - PlusBar
- `-|` - MinusBar
- `*|` - StarBar
- `/|` - SlashBar

### Arithmetic Variants - Percent (%)
Reserved, not yet used in parser.

- `+%` - PlusPercent
- `-%` - MinusPercent
- `*%` - StarPercent
- `/%` - SlashPercent

### Assignment Operators
Reserved, not yet used in parser.

- `+=` - PlusEquals
- `-=` - MinusEquals
- `*=` - StarEquals
- `/=` - SlashEquals

### Range
- `..` - DotDot: Reserved, not yet used in parser. A float's point is a
  single `.`, so `1.5` is unaffected.

### Comparison Operators
- `.<` - DotLess: Less than
- `.>` - DotGreater: Greater than
- `<=` - LessEquals: Less than or equal
- `>=` - GreaterEquals: Greater than or equal
- `==` - EqualsEquals: Equality
- `!=` - ExclamationEquals: Not equal

### Other
- `:-` - ColonDash: Reserved, not yet used in parser

## Three-Character Sigils

### Assignment Variants - Optional (?)
Reserved, not yet used in parser.

- `+?=` - PlusQuestionEquals
- `-?=` - MinusQuestionEquals
- `*?=` - StarQuestionEquals
- `/?=` - SlashQuestionEquals

### Assignment Variants - Bar (|)
Reserved, not yet used in parser.

- `+|=` - PlusBarEquals
- `-|=` - MinusBarEquals
- `*|=` - StarBarEquals
- `/|=` - SlashBarEquals

## Available Characters

Characters not currently assigned as sigil start characters:

```datalove
& \ ^ `
```

Note: `_` is considered a word character (identifier start).

## Spacing

What a sigil operator attaches to is decided by how it was written: one
against both its neighbours or against neither goes between them, one against
only what follows it is a prefix operator, and one against only what precedes
it is a postfix operator. See
[design-token-gluing.md](design-token-gluing.md).


# Open design questions

Tensor constructors use multi-comma separators (`[| 1 2, 3 4,, 5 6, 7 8 |]`)
instead of semicolons, since `;` is used as a line separator.
