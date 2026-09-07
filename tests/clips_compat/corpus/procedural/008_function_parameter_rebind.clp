;; Rebinding a function parameter changes that invocation value.
;; Level: interaction
;; Covers: +, bind, deffunction, increment
;; Run with load, reset, and run in a fresh environment.

(deffunction increment (?x) (bind ?x (+ ?x 1)) ?x)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (increment 3) crlf))
