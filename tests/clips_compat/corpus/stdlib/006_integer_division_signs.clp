;; Integer division truncates toward zero for negative operands.
;; Level: boundary
;; Covers: div
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (div -7 3) " " (div 7 -3) " " (div -7 -3) crlf))
