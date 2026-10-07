;; A correlated alternative in an ordered sequence binds each split once, under not and exists too.
;; Level: interaction
;; Covers: patterns, field-disjunction, multifield, not, exists, join, variable-binding, salience
(deffacts seed (want b) (key b) (key z) (tags a b c) (tags x b))
(defrule pick
  (declare (salience 20))
  (want ?k)
  (tags $?before ?t&?k|x $?)
  => (printout t "pick " ?k " " ?t " after " (length$ ?before) crlf))
(defrule has
  (declare (salience 10))
  (key ?k)
  (exists (tags $? ?k|c $?))
  => (printout t "has " ?k crlf))
(defrule lacks
  (key ?k)
  (not (tags $? ?k|y $?))
  => (printout t "lacks " ?k crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
