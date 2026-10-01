;; Builtin and equivalent deffunction sort predicates agree on direction.
;; Level: interaction
;; Covers: <, >, create$, deffunction, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction descending (?a ?b) (< ?a ?b))

(deffunction ascending (?a ?b) (> ?a ?b))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort < (create$ 3 1 2 1)) " " (sort descending (create$ 3 1 2 1)) crlf)
    (printout t (sort > (create$ 3 1 2 1)) " " (sort ascending (create$ 3 1 2 1)) crlf))
