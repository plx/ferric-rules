;; Lexeme predicates distinguish SYMBOL, STRING, and numeric values.
;; Level: boundary
;; Covers: lexemep, stringp, symbolp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (symbolp red) " " (symbolp "red") " " (stringp "red") " " (stringp red) " " (lexemep red) " " (lexemep "red") " " (lexemep 2) crlf))
