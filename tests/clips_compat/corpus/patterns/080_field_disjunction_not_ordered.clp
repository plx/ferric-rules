;; Either disjunct blocks one negated condition.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, ordered-facts
(deffacts seed (go) (sym a))
(defrule match (go) (not (sym a|b)) => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
