;; Subtraction applies each subsequent operand to the running value.
;; Level: basic
;; Covers: -
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (- 20 3 2) crlf))
