;; Foreach visits each multifield element in order.
;; Level: basic
;; Covers: create$, foreach
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (foreach ?item (create$ a 2 c) (printout t ?item crlf)))
