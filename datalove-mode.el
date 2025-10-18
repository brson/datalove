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
    (modify-syntax-entry ?$ "'" table)
    (modify-syntax-entry ?? "'" table)
    (modify-syntax-entry ?: "." table)
    (modify-syntax-entry ?, "." table)
    (modify-syntax-entry ?. "." table)

    table)
  "Syntax table for `datalove-mode'.")

(defvar datalove-font-lock-keywords
  (let* ((keywords
          '("let" "var" "nil" "bool" "true" "false"
            "u8" "u16" "u32" "u64" "i8" "i16" "i32" "i64"
            "f32" "f64" "int" "float"
            "string" "opt" "struct" "error" "tuple" "enum"
            "token" "map" "set" "list"
            "fun" "proc" "arena" "block"
            "require" "module" "data"
            "ret" "loop" "break" "continue"
            "if" "else" "end"
            "call" "memoize"
            "not" "and" "or" "xor" "implies"
            "move" "copy" "ref" "out"))
         (keyword-regexp (regexp-opt keywords 'words)))

    `(
      ;; Keywords.
      (,keyword-regexp . font-lock-keyword-face)

      ;; Multi-word keywords.
      ("\\<end\s+\\(fun\\|proc\\|arena\\|if\\)\\>" . font-lock-keyword-face)
      ("\\<require\s+\\(module\\|data\\)\\>" . font-lock-keyword-face)

      ;; Function definitions.
      ("\\<fun\s+\\([a-zA-Z_][a-zA-Z0-9_]*\\)" 1 font-lock-function-name-face)
      ("\\<proc\s+\\([a-zA-Z_][a-zA-Z0-9_]*\\)" 1 font-lock-function-name-face)

      ;; Type annotations and struct/enum names.
      ("\\<struct\s+\\([A-Z][a-zA-Z0-9_]*\\)" 1 font-lock-type-face)
      ("\\<enum\s+\\([A-Z][a-zA-Z0-9_]*\\)" 1 font-lock-type-face)
      ("@\\(struct\\|enum\\)\s+\\([A-Z][a-zA-Z0-9_]*\\)" 2 font-lock-type-face)

      ;; Type names in type positions.
      ("@\\(u8\\|u16\\|u32\\|u64\\|i8\\|i16\\|i32\\|i64\\|f32\\|f64\\|int\\|float\\|bool\\|string\\)\\>" 1 font-lock-type-face)

      ;; Boolean literals.
      ("@\\(true\\|false\\|nil\\)\\>" . font-lock-constant-face)

      ;; Numeric literals.
      ("@\\([0-9]+\\(?:\\.[0-9]+\\)?\\)" . font-lock-constant-face)

      ;; String literals.
      ("@\"\\(?:[^\"\\]\\|\\\\.\\)*\"" . font-lock-string-face)

      ;; Operators.
      ("\\(\\.\\+\\|\\.\\-\\|\\.\\*\\|\\./\\|\\.<\\|\\.>\\|\\.<=\\|\\.>=\\|\\.==\\|\\.!=\\)" . font-lock-builtin-face)

      ;; Sigils.
      ("\\(@\\|#\\|\\$\\|?\\)" . font-lock-variable-name-face)

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
