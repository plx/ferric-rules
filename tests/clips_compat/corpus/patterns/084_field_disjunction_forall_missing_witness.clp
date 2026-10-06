;; Forall checks every item admitted by either disjunct.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, variable-binding
(deffacts seed (go) (item a) (item b) (ok a))
(defrule match (go) (forall (item ?x&a|b) (ok ?x))
  => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
