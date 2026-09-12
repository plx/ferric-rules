;; Exponential and logarithms at exact reference points.
;; Level: basic
;; Covers: exp, log, log10
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (exp 0) " " (log 1) " " (log10 100) crlf))
