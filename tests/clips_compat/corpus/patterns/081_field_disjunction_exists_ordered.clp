;; Different witnesses satisfy one existential condition once.
;; Level: interaction
;; Covers: patterns, field-disjunction, exists, ordered-facts
(deffacts seed (go) (sym a) (sym b))
(defrule match (go) (exists (sym a|b)) => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
