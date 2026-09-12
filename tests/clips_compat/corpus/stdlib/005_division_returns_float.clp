;; Division of integers returns FLOAT even when evenly divisible.
;; Level: basic
;; Covers: /, floatp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (/ 8 2) " " (floatp (/ 8 2)) crlf))
