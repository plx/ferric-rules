;; A correlated alternative in a template multislot drives not, exists, and forall.
;; Level: interaction
;; Covers: patterns, field-disjunction, deftemplate, multislot, not, exists, forall, join, salience
(deftemplate item (slot id) (multislot tags))
(deffacts seed
  (want b) (key b) (key c) (key z)
  (item (id 1) (tags a b))
  (item (id 2) (tags c z)))
(defrule pick
  (declare (salience 30))
  (want ?k)
  (item (id ?id) (tags $? ?t&?k|a $?))
  => (printout t "pick " ?k " " ?id " " ?t crlf))
(defrule has
  (declare (salience 20))
  (key ?k)
  (exists (item (tags ?k|q $?)))
  => (printout t "has " ?k crlf))
(defrule lacks
  (declare (salience 10))
  (key ?k)
  (not (item (tags $? ?k|q)))
  => (printout t "lacks " ?k crlf))
(defrule every
  (key ?k)
  (forall (item (id ?id)) (item (id ?id) (tags $? ?k|c $?)))
  => (printout t "every " ?k crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
