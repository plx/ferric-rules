;; Minimum returns the selected operand including its numeric type.
;; Level: boundary
;; Covers: integerp, min
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (min 4.0 2 8.0) " " (integerp (min 4.0 2 8.0)) crlf))
