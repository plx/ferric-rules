;; Double negation preserves a correlated field disjunction.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, join
(deffacts seed (key a) (key c) (sym a) (sym b) (sym d))
(defrule match (key ?k) (not (not (sym ?k|b)))
  => (printout t "present " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
