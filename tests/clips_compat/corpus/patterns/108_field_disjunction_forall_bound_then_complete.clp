;; Forall then-clause witnesses can match either correlated or literal alternatives.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, join, variable-binding
(deffacts seed (key a) (item a) (item b) (ok a) (ok b))
(defrule match (key ?k) (forall (item ?x) (ok ?x&?k|b))
  => (printout t "complete " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
