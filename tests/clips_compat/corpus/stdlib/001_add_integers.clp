;; Addition preserves INTEGER for integer operands.
;; Level: basic
;; Covers: +
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (+ -4 0 9) crlf))
