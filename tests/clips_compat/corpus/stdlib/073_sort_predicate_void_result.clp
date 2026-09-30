;; A sort predicate without a return value still requests an exchange.
;; Level: boundary
;; Covers: create$, deffunction, printout, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction exchange (?a ?b) (printout t "called;"))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort exchange (create$ 3 1 2)) crlf))
