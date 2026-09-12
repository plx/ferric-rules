;; Power and square root produce FLOAT at exact integer-valued results.
;; Level: basic
;; Covers: **, sqrt
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (** 2 3) " " (sqrt 16) crlf))
