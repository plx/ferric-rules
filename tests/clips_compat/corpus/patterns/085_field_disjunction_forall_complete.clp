;; Both alternatives and their witnesses produce one forall activation.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, variable-binding
(deffacts seed (go) (item a) (item b) (ok a) (ok b))
(defrule match (go) (forall (item ?x&a|b) (ok ?x))
  => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
