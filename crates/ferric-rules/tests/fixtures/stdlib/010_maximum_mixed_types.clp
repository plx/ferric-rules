;; Maximum returns the selected operand including its numeric type.
;; Level: boundary
;; Covers: integerp, max
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (max 4.0 2 8) " " (integerp (max 4.0 2 8)) crlf))
