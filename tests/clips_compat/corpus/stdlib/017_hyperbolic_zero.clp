;; Hyperbolic and inverse hyperbolic functions at exact reference points.
;; Level: boundary
;; Covers: acosh, asinh, atanh, cosh, sinh, tanh
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sinh 0) " " (cosh 0) " " (tanh 0) " " (asinh 0) " " (acosh 1) " " (atanh 0) crlf))
