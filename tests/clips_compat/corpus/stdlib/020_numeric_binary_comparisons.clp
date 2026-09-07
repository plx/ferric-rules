;; Binary numeric comparisons accept mixed INTEGER and FLOAT operands.
;; Level: basic
;; Covers: <, <=, <>, =, >, >=
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (= 2 2.0) " " (< 1 2) " " (> 3 2) " " (<= 2 2.0) " " (>= 2 2.0) " " (<> 1 2) crlf))
