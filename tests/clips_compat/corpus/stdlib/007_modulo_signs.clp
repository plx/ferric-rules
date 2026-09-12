;; Remainder follows the dividend sign.
;; Level: boundary
;; Covers: mod
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (mod -7 3) " " (mod 7 -3) " " (mod -7 -3) crlf))
