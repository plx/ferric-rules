;; A FLOAT operand promotes addition to FLOAT.
;; Level: basic
;; Covers: +, floatp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (floatp (+ 1 2.0)) " " (+ 1 2.0) crlf))
