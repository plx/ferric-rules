;; A leading binding remains available on the RHS for every alternative.
;; Level: interaction
;; Covers: patterns, field-disjunction, variable-binding
(deffacts seed (sym a) (sym b) (sym c))
(defrule match (sym ?x&a|b) => (printout t "P " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
