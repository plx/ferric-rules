;; Value equality distinguishes numeric and lexeme types.
;; Level: boundary
;; Covers: eq, neq
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (eq 2 2.0) " " (eq red "red") " " (eq red red) " " (neq 2 2.0) crlf))
