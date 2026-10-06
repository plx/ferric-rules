;; Forall checks both domain alternatives and ignores unrelated domain facts.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, join, variable-binding
(deffacts seed (key a) (sym a) (sym b) (sym c) (ok a) (ok b))
(defrule match (key ?k) (forall (sym ?x&?k|b) (ok ?x))
  => (printout t "complete " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
