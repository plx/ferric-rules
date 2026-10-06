;; A leading variable binds even when a negative alternative overlaps a literal.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, variable-binding
(deffacts seed (sym a) (sym b) (sym c))
(defrule match (sym ?x&~a|b) => (printout t "P " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
