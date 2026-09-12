;; Symbol concatenation accepts strings and numbers and returns SYMBOL.
;; Level: basic
;; Covers: sym-cat, symbolp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sym-cat a "b" 3) " " (symbolp (sym-cat a "b")) crlf))
