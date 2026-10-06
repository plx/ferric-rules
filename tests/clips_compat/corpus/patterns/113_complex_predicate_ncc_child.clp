;; A nonlinear predicate in an NCC child is evaluated within the negated conjunction.
;; Level: interaction
;; Covers: patterns, not, and, ncc, predicate-constraint, join, assert, retract, salience
(deffacts seed (key 1) (key 4) (key 9) (val 2) (val 3) (mark 3) (phase 1))
(defrule clear
  (key ?k)
  (not (and (val ?x&:(> (* ?x ?x) ?k)) (mark ?x)))
  => (printout t "clear " ?k crlf))
;; A second blocker for already-blocked keys creates no activation.
(defrule add-mark
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (mark 2) (phase 2)) (printout t "added mark 2" crlf))
;; Only the key whose remaining witness fails the predicate becomes clear.
(defrule remove-mark
  (declare (salience -5))
  ?phase <- (phase 2)
  ?mark <- (mark 3)
  => (retract ?phase ?mark) (printout t "removed mark 3" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
