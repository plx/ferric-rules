;; Repeated alternatives do not duplicate activations.
;; Level: interaction
;; Covers: patterns, field-disjunction, ordered-facts
(deffacts seed (sym b))
(defrule match (sym b|b) => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
