;; A correlated predicate disjunction filters the complete negative conjunction.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, join, ncc
(deffacts seed (key a) (key c) (sym a) (sym d) (marker))
(defrule match (key ?k) (not (and (sym ?k|b) (marker)))
  => (printout t "absent " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
