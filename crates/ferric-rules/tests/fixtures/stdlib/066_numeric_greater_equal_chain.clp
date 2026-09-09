;; Greater-or-equal accepts a nonincreasing sequence.
;; Level: boundary
;; Covers: >=
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (>= 3 2 2) " " (>= 3 2 3) crlf))
