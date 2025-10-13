(define-generic-mode 'la2-mode
  '(";") 
  '(
    "let"
    "var"
    "nil"
    "bool"
    "true"
    "false"
    "u32"
    "string"
    "opt"
    "struct"
    "error"
    "tuple"
    "enum"
    "token"
    "map"
    "set"
    "fun"
    "end fun"
    "proc"
    "end proc"
    "arena"
    "end arena"
    "block"
    "require module"
    "require data"
    "ret"
    "loop"
    "break"
    "continue"
    "if"
    "end if"
    "else"
    "call"
    "memoize"
    "not" "and" "or" "xor" "implies"
    "move" "copy" "ref" "out"
    )
  '(
    ("@" . 'font-lock-variable-name-face)
    ("#" . 'font-lock-variable-name-face)
    ("$" . 'font-lock-variable-name-face)
    ("#" . 'font-lock-variable-name-face)
    ("%" . 'font-lock-variable-name-face)
    ("?" . 'font-lock-variable-name-face)
    (":" . 'font-lock-variable-name-face)
    ("," . 'font-lock-variable-name-face)
    ("(" . 'font-lock-constant-face)
    (")" . 'font-lock-constant-face)
    ("{" . 'font-lock-constant-face)
    ("}" . 'font-lock-constant-face)
    ("[" . 'font-lock-constant-face)
    ("]" . 'font-lock-constant-face)
    ("<" . 'font-lock-constant-face)
    (">" . 'font-lock-constant-face)
    )
  '(".dlt\\'" ".dfs\\'" ".dfm\\'" ".dls\\'" ".dlm\\'")
  "datalove language")
