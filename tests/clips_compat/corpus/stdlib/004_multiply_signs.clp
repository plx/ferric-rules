;; Multiplication handles several operands and a zero operand.
;; Level: boundary
;; Covers: *
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (* -2 -3 4) " " (* -2 0 4) crlf))
