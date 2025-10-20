# Sigil Assignments

Current lexer sigils and their uses in the language.

## Single-Character Sigils

### Punctuation
- `.` - Dot
  - Member access in import statements: `import u32.negate`
  - Part of comparison operators: `.<`, `.>`

- `,` - Comma
  - Separating items in lists, tuples, function parameters, etc.

- `;` - Semicolon
  - Statement separator (alternative to newline)

### Heap Allocation
- `@` - At
  - Local heap allocation sigil
  - Type hint: `let x: @u32`
  - Value literal: `let x = @42`

- `#` - Hash
  - Global heap allocation sigil
  - Type hint: `let x: #u32`
  - Value literal: `let x = #42`

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
  - Path separator in module paths: `sys/std/u32`
  - OVERLOAD: also in type hints
  - OVERLOAD: also in require statements.

### Structural
- `:` - Colon
  - Type hint separator: `let x: u32`
  - Function parameter types: `fun foo(x: u32)`
  - Function return types: `fun foo(): u32`

- `=` - Equals
  - Assignment in let statements: `let x = value`

- `|` - Pipe
  - Binding delimiter in if statements: `if condition |x|`

### Braces (Balanced Pairs)
- `(` `)` - ParenOpen, ParenClose
  - Function parameters: `fun foo(a: u32, b: u32)`
  - Function calls: `foo(1, 2)`
  - Tuples: `(1, 2, 3)`
  - Expression grouping

- `{` `}` - BraceOpen, BraceClose
  - Maps: `@map { @1 = @10, @2 = @20 }`
  - Sets: `@set { @1, @2, @3 }`
  - Structs: `@struct { x = @1, y = @2 }`

- `[` `]` - BracketOpen, BracketClose
  - Lists: `@[1, 2, 3]`
  - List types: `[u32]`

- `<` `>` - AngleOpen, AngleClose
  - Type parameters for collections
  - Map types: `map <key_type, value_type>`
  - Set types: `set <element_type>`
  - List types (verbose): `list <element_type>` (alternative to `[element_type]`)

## Two-Character Sigils

### Arithmetic Variants - Optional (?)
Return Option on overflow/error instead of panicking.

- `+?` - PlusQuestion: Optional addition
- `-?` - MinusQuestion: Optional subtraction
  - OVERLOAD: Also unary optional negation: `-?expr`
- `*?` - StarQuestion: Optional multiplication
- `/?` - SlashQuestion: Optional division

### Arithmetic Variants - Checked (!)
Return Result on overflow/error instead of panicking.

- `+!` - PlusExclamation: Checked addition
- `-!` - MinusExclamation: Checked subtraction
  - OVERLOAD: Also unary checked negation: `-!expr`
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