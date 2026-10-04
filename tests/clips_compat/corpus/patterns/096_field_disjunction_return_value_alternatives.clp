;; Overlapping return-value alternatives do not duplicate a fact match.
;; Level: interaction
;; Covers: patterns, field-disjunction, return-value-constraint, variable-binding
(deffacts seed (sym 3) (sym 4))
(defrule match (sym ?x&=(+ 1 2)|=(+ 2 1))
  => (printout t "P " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
