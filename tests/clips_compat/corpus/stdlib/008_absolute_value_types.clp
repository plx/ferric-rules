;; Absolute value preserves its numeric argument type.
;; Level: boundary
;; Covers: abs, floatp, integerp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (abs -4) " " (abs -4.0) " " (integerp (abs -4)) " " (floatp (abs -4.0)) crlf))
