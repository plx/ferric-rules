;; A nonlinear predicate in the forall condition selects which facts need a witness.
;; Level: interaction
;; Covers: patterns, forall, predicate-constraint, join, assert, retract, salience
(deffacts seed (key 1) (key 4) (val 2) (val 3) (mark 3) (phase 1))
(defrule all
  (key ?k)
  (forall (val ?x&:(> (* ?x ?x) ?k)) (mark ?x))
  => (printout t "all " ?k crlf))
;; The missing witness only matters for keys whose predicate admits val 2.
(defrule add-mark
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (mark 2) (phase 2)) (printout t "added mark 2" crlf))
(defrule add-val
  (declare (salience -5))
  ?phase <- (phase 2)
  => (retract ?phase) (assert (val 5) (phase 3)) (printout t "added val 5" crlf))
;; Adding the missing witness restores the condition for the remaining key.
(defrule add-last-mark
  (declare (salience -5))
  ?phase <- (phase 3)
  ?key <- (key 4)
  => (retract ?phase ?key) (assert (mark 5)) (printout t "added mark 5" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
