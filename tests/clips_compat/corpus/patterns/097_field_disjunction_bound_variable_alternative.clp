;; A disjunction can join an earlier binding or match a literal.
;; Level: interaction
;; Covers: patterns, field-disjunction, join, variable-binding
(deffacts seed (key a) (sym a) (sym b) (sym c))
(defrule match (key ?k) (sym ?x&?k|b)
  => (printout t "P " ?x crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
