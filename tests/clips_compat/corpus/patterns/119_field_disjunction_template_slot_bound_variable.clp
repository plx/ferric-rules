;; A correlated alternative in a template single slot binds the slot for the RHS and joins.
;; Level: interaction
;; Covers: patterns, field-disjunction, deftemplate, not, exists, join, variable-binding, salience
(deftemplate cell (slot v) (slot w))
(deffacts seed
  (key 1) (key 5)
  (cell (v 1) (w x))
  (cell (v 99) (w y))
  (cell (v 3) (w z)))
(defrule pick
  (declare (salience 20))
  (key ?k)
  (cell (v ?v&?k|99) (w ?w))
  => (printout t "pick " ?k " " ?v " " ?w crlf))
(defrule has
  (declare (salience 10))
  (key ?k)
  (exists (cell (v ?k|3)))
  => (printout t "has " ?k crlf))
(defrule lacks
  (key ?k)
  (not (cell (v ?k|4)))
  => (printout t "lacks " ?k crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
