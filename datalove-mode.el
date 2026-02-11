;;; datalove-mode.el --- Major mode for editing Datalove files

;;; Commentary:
;; Major mode for editing Datalove language files (.dlt, .dfs, .dfm, .dls, .dlm)

;;; Code:

(defvar datalove-mode-syntax-table
  (let ((table (make-syntax-table)))
    ;; Line comments.
    (modify-syntax-entry ?/ ". 124" table)
    (modify-syntax-entry ?* ". 23b" table)
    (modify-syntax-entry ?\n ">" table)

    ;; Strings.
    (modify-syntax-entry ?\" "\"" table)

    ;; Punctuation and operators.
    (modify-syntax-entry ?@ "'" table)
    (modify-syntax-entry ?# "'" table)
    (modify-syntax-entry ?? "." table)
    (modify-syntax-entry ?! "." table)
    (modify-syntax-entry ?: "." table)
    (modify-syntax-entry ?, "." table)
    (modify-syntax-entry ?. "." table)
    (modify-syntax-entry ?| "." table)

    table)
  "Syntax table for `datalove-mode'.")

(defvar datalove-font-lock-keywords
  (let* ((keywords
          '("let" "var"
            "fun" "ret"
            "require" "data" "import"
            "if" "else" "end"
            "loop" "while" "for" "break" "continue"
            "match" "case" "default"
            "atom" "term"
            "not" "and" "or" "xor"
            "some" "ok" "er" "none"
            "ref" "mut" "out" "in"
            ;; Types.
            "bool" "true" "false"
            "u8" "u16" "u32" "u64" "i8" "i16" "i32" "i64"
            "f32" "int"
            "string" "opt" "error" "tuple" "enum"
            "list" "tensor" "table"))
         (keyword-regexp (regexp-opt keywords 'words)))

    `(
      ;; Keywords.
      (,keyword-regexp . font-lock-keyword-face)

      ;; Multi-word keywords.
      ("\\<end\s+\\(fun\\|if\\|loop\\|match\\)\\>" . font-lock-keyword-face)
      ("\\<require\s+\\(data\\)\\>" . font-lock-keyword-face)
      ("\\<loop\s+while\\>" . font-lock-keyword-face)
      ("\\<case\s+\\(atom\\|term\\|default\\)\\>" . font-lock-keyword-face)

      ;; Function definitions.
      ("\\<fun\s+\\([a-zA-Z_][a-zA-Z0-9_]*\\)" 1 font-lock-function-name-face)

      ;; Atom and term variant names.
      ("\\<atom\s+\\([A-Z][a-zA-Z0-9_]*\\)" 1 font-lock-type-face)
      ("\\<term\s+\\([A-Z][a-zA-Z0-9_]*\\)" 1 font-lock-type-face)

      ;; Type annotations and enum names.
      ("\\<enum\s+{" . font-lock-keyword-face)

      ;; Type names in type positions.
      ("@\\(u8\\|u16\\|u32\\|u64\\|i8\\|i16\\|i32\\|i64\\|f32\\|int\\|bool\\|string\\)\\>" 1 font-lock-type-face)

      ;; Boolean and special literals.
      ("@\\(true\\|false\\|none\\)\\>" . font-lock-constant-face)

      ;; Numeric literals (decimal and hex).
      ("@\\(0x[0-9a-fA-F]+\\|[0-9]+\\(?:\\.[0-9]+\\)?\\)" . font-lock-constant-face)

      ;; @data, @error, @tensor constructors.
      ("@\\(data\\|error\\|tensor\\)\\>" . font-lock-builtin-face)

      ;; Map (%{) and set (#{) sigils.
      ("%{" . font-lock-builtin-face)
      ("#{" . font-lock-builtin-face)

      ;; Table delimiters.
      ("{|\\||}" . font-lock-builtin-face)

      ;; String literals.
      ("@\"\\(?:[^\"\\]\\|\\\\.\\)*\"" . font-lock-string-face)

      ;; Comparison operators.
      ("\\(\\.<\\|\\.>\\|<=\\|>=\\|==\\|!=\\)" . font-lock-builtin-face)

      ;; Checked/optional arithmetic operators.
      ("\\([+\\-*/][!?]\\)" . font-lock-builtin-face)

      ;; Heap sigils.
      ("\\(@\\|#\\)" . font-lock-variable-name-face)

      ;; Type annotation colon.
      (":" . font-lock-keyword-face)
      ))
  "Font lock keywords for `datalove-mode'.")

(defvar datalove-mode-map
  (let ((map (make-sparse-keymap)))
    map)
  "Keymap for `datalove-mode'.")

;;;###autoload
(define-derived-mode datalove-mode prog-mode "Datalove"
  "Major mode for editing Datalove language files."
  :syntax-table datalove-mode-syntax-table

  ;; Font lock.
  (setq font-lock-defaults '(datalove-font-lock-keywords))

  ;; Comments.
  (setq-local comment-start "// ")
  (setq-local comment-end "")
  (setq-local comment-start-skip "//+\\s-*\\|/\\*+\\s-*")

  ;; Indentation.
  (setq-local indent-tabs-mode nil)
  (setq-local tab-width 2))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dlt\\'" . datalove-mode))
(add-to-list 'auto-mode-alist '("\\.dfs\\'" . datalove-mode))
(add-to-list 'auto-mode-alist '("\\.dfm\\'" . datalove-mode))
(add-to-list 'auto-mode-alist '("\\.dls\\'" . datalove-mode))
(add-to-list 'auto-mode-alist '("\\.dlm\\'" . datalove-mode))

(provide 'datalove-mode)

;;; datalove-mode.el ends here
