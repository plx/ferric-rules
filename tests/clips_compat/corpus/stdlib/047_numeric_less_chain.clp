;; Less-than accepts a strictly ascending sequence.
;; Level: boundary
;; Covers: <
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (< 1 2 3) " " (< 1 2 2) crlf))
