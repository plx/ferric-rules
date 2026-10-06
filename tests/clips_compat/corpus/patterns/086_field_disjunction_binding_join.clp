;; The second alternative binds the same variable used by a later pattern.
;; Level: interaction
;; Covers: patterns, field-disjunction, join, variable-binding
(deffacts seed (sym b) (other c))
(defrule match (sym ?x&a|b) (other ?x)
  => (printout t "match " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
