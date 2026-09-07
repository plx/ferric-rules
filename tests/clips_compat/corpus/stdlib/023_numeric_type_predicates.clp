;; Numeric predicates distinguish INTEGER, FLOAT, and nonnumeric values.
;; Level: boundary
;; Covers: floatp, integerp, numberp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (integerp 2) " " (integerp 2.0) " " (floatp 2.0) " " (floatp 2) " " (numberp 2.0) " " (numberp two) crlf))
