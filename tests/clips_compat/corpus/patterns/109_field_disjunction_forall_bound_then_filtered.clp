;; A matching item value fails when neither then-clause alternative admits it.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, join, variable-binding
(deffacts seed (key c) (item a) (ok a))
(defrule match (key ?k) (forall (item ?x) (ok ?x&?k|b))
  => (printout t "complete " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
