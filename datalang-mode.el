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
    "proc"
    "end fun"
    "end proc"
    "require module"
    "require data"
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
  '(".dle\\'" ".dlm\\'" ".dls\\'")
  "datalang language")
