;; Sort evaluates its predicate name, then its data, then calls the predicate;
;; empty and singleton data never call it.
;; Level: interaction
;; Covers: *, +, /, >, bind, create$, defglobal, deffunction, sort
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*trace* = 0)

(deffunction mark (?digit ?value)
    (bind ?*trace* (+ (* ?*trace* 10) ?digit))
    ?value)

(deffunction exchange (?a ?b)
    (bind ?*trace* (+ (* ?*trace* 10) 4))
    (> ?a ?b))

(deffunction fail (?a ?b) (/ 1 0))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (bind ?result (sort (mark 1 exchange) (mark 2 (create$ 3 1)) (mark 3 2)))
    (printout t ?result " " ?*trace* crlf)
    (bind ?*trace* 0)
    (bind ?result (sort (mark 1 >) (mark 2 (create$ 3 1)) (mark 3 2)))
    (printout t ?result " " ?*trace* crlf)
    (printout t (create$ (sort fail) (sort fail 7)) crlf))
