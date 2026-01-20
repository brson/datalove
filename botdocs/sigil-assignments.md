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

### Reserved (Previously Heap Allocation)
- `@` - At
  - Currently unused (previously local heap sigil)

- `#` - Hash
  - Currently unused (previously global heap sigil)

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
  - Map entry assignment: `map { 1 = 10 }`
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
  - Maps: `map { 1 = 10, 2 = 20 }`
  - Sets: `set { 1, 2, 3 }`
  - Structs: `{ x = 1, y = 2 }`
  - Enum type hints: `enum { A, B(u32) }`
  - Anonymous struct type hints: `{ x: u32 }`

- `[` `]` - BracketOpen, BracketClose
  - Lists: `[1, 2, 3]`
  - List types: `[u32]`
  - Tensor shape: `tensor [2, 3] [...]`
  - Tensor data: `tensor [...] [1, 2, 3, 4]`

- `<` `>` - AngleOpen, AngleClose
  - Type parameters for collections
  - Map types: `map <key_type, value_type>`
  - Set types: `set <element_type>`
  - Tensor types: `tensor <element_type, rank>`

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

```
$ % & \ ^ ` ~
```

Note: `_` is considered a word character (identifier start).


# Open design questions

`;` is nice for tensor constructor row separators,
but we use them for line-separators.
