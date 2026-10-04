;;; datalove-mode.el --- Major mode for editing Datalove files -*- lexical-binding: t; -*-

;;; Commentary:
;; Major mode for editing Datalove language files (.dlt, .dfs, .dfm, .dls, .dlm).
;;
;; Datalove is a tower: datalit (.dlt) is the data literal language, datafun
;; (.dfs scripts, .dfm modules) adds functions, and full Datalove (.dls, .dlm)
;; adds procedures and objects.

;;; Code:

(defvar datalove-mode-syntax-table
  (let ((table (make-syntax-table)))
    ;; Comments: `//' to end of line, and nestable `/* */'.
    (modify-syntax-entry ?/ ". 124b" table)
    (modify-syntax-entry ?* ". 23n" table)
    (modify-syntax-entry ?\n "> b" table)

    ;; Strings, with backslash escapes.
    (modify-syntax-entry ?\" "\"" table)
    (modify-syntax-entry ?\\ "\\" table)

    ;; Underscores are word constituents in names.
    (modify-syntax-entry ?_ "_" table)

    ;; Sigils. None of these open or close anything on their own; the paired
    ;; delimiters are the ASCII brackets, which the standard table already
    ;; gives paren syntax, and the pipe that earmuffs them is punctuation.
    (dolist (char '(?@ ?# ?% ?$ ?~ ?? ?! ?: ?\; ?, ?. ?| ?+ ?- ?= ?< ?>))
      (modify-syntax-entry char "." table))

    table)
  "Syntax table for `datalove-mode'.")

(defconst datalove-mode--keywords
  '("let" "var" "const" "set" "call"
    "fun" "ret" "native"
    "require" "module" "import" "rider"
    "if" "else" "end"
    "loop" "while" "break" "continue"
    "match" "case" "default"
    "type" "with" "is"
    "atom" "term" "enum" "tuple"
    "ref" "mut" "out"
    "not" "and" "or" "xor"
    "debuglog")
  "Words the datafun parser reads as keywords.")

(defconst datalove-mode--types
  '("bool"
    "u8" "u16" "u32" "u64"
    "i8" "i16" "i32" "i64"
    "index" "offset"
    "f32" "f64" "int"
    "string")
  "Primitive type names.")

(defconst datalove-mode--builtins
  '("data" "error"
    "some" "ok" "er" "none"
    "icall")
  "Value constructors and the intrinsic-call marker.

`data' and `error' name types as well as build values, so they get one
face in both positions.")

(defconst datalove-mode--bounds
  '("float" "fixedint" "ord")
  "Bounds a type parameter can be given in a `with' clause.")

(defconst datalove-mode--name-re
  "[A-Za-z_][A-Za-z0-9_]*"
  "Regexp matching one identifier.")

(defvar datalove-font-lock-keywords
  `(;; Definitions, before the bare keyword lists, so the defined name keeps
    ;; the face its definition gives it.
    (,(concat "\\_<\\(?:native[ \t]+\\)?fun[ \t]+\\(" datalove-mode--name-re "\\)")
     1 font-lock-function-name-face)
    (,(concat "\\_<\\(?:native[ \t]+\\)?fun[ \t]+" datalove-mode--name-re
              "[ \t]*<\\([^<>\n]*\\)>")
     1 font-lock-type-face)
    (,(concat "\\_<type[ \t]+\\(" datalove-mode--name-re "\\)")
     1 font-lock-type-face)
    (,(concat "\\_<\\(?:let\\|var\\|const\\)[ \t]+\\(" datalove-mode--name-re "\\)")
     1 font-lock-variable-name-face)
    (,(concat "\\_<icall[ \t]+\\(" datalove-mode--name-re "\\)")
     1 font-lock-function-name-face)

    ;; `require module lib/pkg/module' and `import module.name'.
    (,(concat "\\_<require[ \t]+module[ \t]+\\("
              datalove-mode--name-re "/" datalove-mode--name-re "/"
              datalove-mode--name-re "\\)")
     1 font-lock-constant-face)
    (,(concat "\\_<import[ \t]+\\(" datalove-mode--name-re "\\)\\.\\("
              datalove-mode--name-re "\\)")
     (1 font-lock-constant-face)
     (2 font-lock-function-name-face))

    ;; Enum variants: `atom Name' and `term Name Payload', in both type hints
    ;; and expressions, plus the binding a `case term' arm introduces.
    (,(concat "\\_<case[ \t]+term[ \t]+" datalove-mode--name-re "[ \t]+\\("
              datalove-mode--name-re "\\)")
     1 font-lock-variable-name-face)
    (,(concat "\\_<\\(?:atom\\|term\\)[ \t]+\\(" datalove-mode--name-re "\\)")
     1 font-lock-type-face)

    ;; The name a bound is given: `with { T is ord, }'.
    (,(concat "\\_<is[ \t]+\\(" (regexp-opt datalove-mode--bounds) "\\)\\_>")
     1 font-lock-type-face)

    ;; The binding an `if' or `else' destructures: `if opt |value|'.
    (,(concat "[ \t]|\\(" datalove-mode--name-re "\\)|[ \t]*$")
     1 font-lock-variable-name-face)

    ;; Word lists.
    (,(regexp-opt datalove-mode--types 'symbols) . font-lock-type-face)
    (,(regexp-opt '("true" "false") 'symbols) . font-lock-constant-face)
    (,(regexp-opt datalove-mode--builtins 'symbols) . font-lock-builtin-face)
    (,(regexp-opt datalove-mode--keywords 'symbols) . font-lock-keyword-face)

    ;; Numeric literals, hex and decimal. A float is written with a `.', which
    ;; lexes as its own sigil, so the two halves match separately.
    ("\\_<0x[0-9a-fA-F]+\\_>" . font-lock-constant-face)
    ("\\_<[0-9]+\\_>" . font-lock-constant-face)

    ;; Collection sigils: maps, sets, tables, tensors, and the remaining
    ;; earmuffed brackets the lexer knows.
    ("%{\\|#{\\|{|\\||}\\|\\[|\\||\\]\\|(|\\||)\\|<|\\||>" . font-lock-builtin-face)

    ;; Checked (`!') and optional (`?') arithmetic, and the comparisons. The
    ;; lexer also knows a saturating `|' and a wrapping `%' form, but no
    ;; parser reads either, so neither is an operator to highlight.
    ("[-+*/][!?]" . font-lock-builtin-face)
    ("\\.<\\|\\.>\\|<=\\|>=\\|==\\|!=" . font-lock-builtin-face))
  "Font lock keywords for `datalove-mode'.")

(defconst datalove-mode--block-open-re
  "[ \t]*\\(?:fun\\|if\\|loop\\|match\\)\\_>"
  "Regexp matching a line that opens a block.

A `native fun' declares a signature and has no body, so it is not here:
the line starts with `native', which this does not match.")

(defconst datalove-mode--block-close-re
  "[ \t]*end\\_>"
  "Regexp matching a line that closes a block.")

(defconst datalove-mode--block-continue-re
  "[ \t]*\\(?:else\\|case\\)\\_>"
  "Regexp matching a line that closes one arm of a block and opens the next.")

(defun datalove-mode--skippable-line-p ()
  "Return non-nil if the current line holds nothing that affects indentation."
  (save-excursion
    (beginning-of-line)
    (or (looking-at-p "[ \t]*$")
        (looking-at-p "[ \t]*/[/*]")
        ;; Inside a string or a block comment that began on an earlier line.
        (nth 8 (syntax-ppss (point))))))

(defun datalove-mode--previous-code-line ()
  "Move point to the previous line that affects indentation.
Return non-nil if there was one."
  (let ((found nil))
    (while (and (not found) (zerop (forward-line -1)))
      (unless (datalove-mode--skippable-line-p)
        (setq found t)))
    found))

(defun datalove-mode--previous-statement-line ()
  "Move point to the line the previous statement starts on.
A statement can run over several lines inside brackets, and the lines
after the first say nothing about where the next statement goes, so they
are passed over.  Return non-nil if there was such a line."
  (let ((found nil))
    (while (and (not found) (datalove-mode--previous-code-line))
      (unless (nth 1 (syntax-ppss (point)))
        (setq found t)))
    found))

(defun datalove-mode--opener-indentation ()
  "Return the indentation of the line opening the block point's line is in.
Return nil if point is not inside a block."
  (save-excursion
    (beginning-of-line)
    (let ((depth 0)
          (result nil))
      (while (and (not result) (datalove-mode--previous-statement-line))
        (cond
         ((looking-at datalove-mode--block-close-re)
          (setq depth (1+ depth)))
         ((looking-at datalove-mode--block-open-re)
          (if (zerop depth)
              (setq result (current-indentation))
            (setq depth (1- depth))))))
      result)))

(defun datalove-mode--calculate-indent ()
  "Return the column the current line should be indented to."
  (save-excursion
    (beginning-of-line)
    (let ((state (syntax-ppss (point))))
      (cond
       ;; Inside a string or a block comment that opened on an earlier line,
       ;; where the whitespace at the front of the line is content.
       ((nth 8 state)
        (current-indentation))

       ;; A continuation line inside brackets. Both hanging and aligned
       ;; continuations are written in Datalove, and nothing in the line tells
       ;; which one it means to be, so an already-indented line is left where
       ;; its author put it. What is decided here is the bracket that closes
       ;; the run, which goes back to the line that opened it, and a line
       ;; sitting at column zero, which has not been indented yet.
       ((nth 1 state)
        (let ((opener (nth 1 state)))
          (cond
           ((looking-at "[ \t]*[])}|]")
            (save-excursion (goto-char opener) (current-indentation)))
           ((> (current-indentation) 0)
            (current-indentation))
           (t
            (save-excursion
              (let ((opener-line (line-number-at-pos opener)))
                (if (datalove-mode--previous-code-line)
                    (if (= (line-number-at-pos) opener-line)
                        (+ (current-indentation) tab-width)
                      (current-indentation))
                  0)))))))

       ;; `end', `else' and `case' line up with the line that opened the block
       ;; they belong to, and their bodies go one level in from there.
       ((or (looking-at datalove-mode--block-close-re)
            (looking-at datalove-mode--block-continue-re))
        (or (datalove-mode--opener-indentation) 0))

       (t
        (save-excursion
          (if (not (datalove-mode--previous-statement-line))
              0
            (let ((previous (current-indentation)))
              (if (or (looking-at datalove-mode--block-open-re)
                      (looking-at datalove-mode--block-continue-re))
                  (+ previous tab-width)
                previous)))))))))

(defun datalove-indent-line ()
  "Indent the current line of Datalove code."
  (interactive)
  (let ((target (datalove-mode--calculate-indent))
        (offset (- (point) (line-beginning-position) (current-indentation))))
    (indent-line-to target)
    (when (> offset 0)
      (goto-char (+ (point) offset)))))

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
  (setq-local comment-start-skip "//+[ \t]*\\|/\\*+[ \t]*")
  (setq-local comment-multi-line t)

  ;; Indentation.
  (setq-local indent-tabs-mode nil)
  (setq-local tab-width 2)
  (setq-local indent-line-function #'datalove-indent-line))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dlt\\'" . datalove-mode))
;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dfs\\'" . datalove-mode))
;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dfm\\'" . datalove-mode))
;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dls\\'" . datalove-mode))
;;;###autoload
(add-to-list 'auto-mode-alist '("\\.dlm\\'" . datalove-mode))

(provide 'datalove-mode)

;;; datalove-mode.el ends here
