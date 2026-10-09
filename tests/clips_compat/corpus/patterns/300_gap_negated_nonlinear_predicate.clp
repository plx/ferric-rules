;; A nonlinear predicate in a directly negated ordered pattern runs in CLIPS.
;; Level: boundary
;; Covers: patterns, not, predicate-constraint, join, retract, salience
(deffacts seed (anchor 2) (data 3) (data 1) (phase 1))
(defrule candidate
  (anchor ?min)
  (not (data ?x&:(> (* ?x ?x) (* ?min ?min))))
  => (printout t "clear " ?min crlf))
(defrule drop-blocker
  (declare (salience -5))
  ?p <- (phase 1)
  ?d <- (data 3)
  => (retract ?p ?d) (printout t "retracted data 3" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
