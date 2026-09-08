;; Less-or-equal accepts a nondecreasing sequence.
;; Level: boundary
;; Covers: <=
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (<= 1 2 2) " " (<= 1 2 1) crlf))
