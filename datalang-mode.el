(define-generic-mode 'la2-mode
  '(";") 
  '(
    "let"
    "nil"
    "bool"
    "true"
    "false"
    "u32"
    "string"
    "opt"
    "struct"
    "tuple"
    "enum"
    "token"
    "map"
    )
  '(
    ("@" . 'font-lock-variable-name-face)
    ("#" . 'font-lock-variable-name-face)
    ("$" . 'font-lock-variable-name-face)
    ("#" . 'font-lock-variable-name-face)
    ("%" . 'font-lock-variable-name-face)
    ("?" . 'font-lock-function-call-face)
    (":" . 'font-lock-function-call-face)
    ("," . 'font-lock-function-call-face)
    ("(" . 'font-lock-constant-face)
    (")" . 'font-lock-constant-face)
    ("{" . 'font-lock-constant-face)
    ("}" . 'font-lock-constant-face)
    ("[" . 'font-lock-constant-face)
    ("]" . 'font-lock-constant-face)
    )
  '(".dl\\'")
  "datalang language")
