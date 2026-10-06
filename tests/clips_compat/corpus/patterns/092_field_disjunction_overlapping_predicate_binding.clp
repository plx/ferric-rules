;; Overlapping predicates share one leading binding and activation.
;; Level: interaction
;; Covers: patterns, field-disjunction, predicate-constraint, variable-binding
(deffacts seed (sym -2) (sym 5) (sym 12))
(defrule match (sym ?x&:(> ?x 0)|:(< ?x 10))
  => (printout t "P " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
