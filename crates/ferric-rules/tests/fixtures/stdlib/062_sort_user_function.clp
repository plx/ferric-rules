;; Sort invokes a user-defined predicate with two elements.
;; Level: interaction
;; Covers: >, create$, deffunction, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction exchange (?left ?right) (> ?left ?right))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort exchange (create$ 3 1 2)) crlf))
