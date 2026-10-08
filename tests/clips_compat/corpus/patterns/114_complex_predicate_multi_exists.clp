;; A nonlinear predicate in a multi-pattern exists is part of each supporting tuple.
;; Level: interaction
;; Covers: patterns, exists, predicate-constraint, join, assert, retract, salience
(deffacts seed (key 1) (key 9) (val 2) (val 3) (mark 3) (phase 1))
(defrule some
  (key ?k)
  (exists (val ?x&:(> (* ?x ?x) ?k)) (mark ?x))
  => (printout t "some " ?k crlf))
;; A tuple that fails the predicate supports nothing.
(defrule add-small
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (mark 2) (phase 2)) (printout t "added mark 2" crlf))
(defrule add-large
  (declare (salience -5))
  ?phase <- (phase 2)
  => (retract ?phase) (assert (val 4) (mark 4)) (printout t "added val 4" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
