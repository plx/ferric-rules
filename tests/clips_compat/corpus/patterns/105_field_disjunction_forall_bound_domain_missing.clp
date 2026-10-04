;; A missing witness for the literal domain alternative makes forall false.
;; Level: interaction
;; Covers: patterns, field-disjunction, forall, join, variable-binding
(deffacts seed (key a) (sym a) (sym b) (sym c) (ok a) (ok c))
(defrule match (key ?k) (forall (sym ?x&?k|b) (ok ?x))
  => (printout t "complete " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
