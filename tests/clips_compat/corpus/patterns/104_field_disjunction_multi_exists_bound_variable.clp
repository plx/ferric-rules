;; Several existential patterns share a correlated disjunctive constraint.
;; Level: interaction
;; Covers: patterns, field-disjunction, exists, join
(deffacts seed (key a) (key c) (sym a) (sym b) (sym d) (marker))
(defrule match (key ?k) (exists (sym ?k|b) (marker))
  => (printout t "present " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
