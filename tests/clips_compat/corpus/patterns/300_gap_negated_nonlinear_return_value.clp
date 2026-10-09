;; A nonlinear return-value constraint in a directly negated ordered pattern runs in CLIPS.
;; Level: boundary
;; Covers: patterns, not, return-value-constraint, join, retract, salience
(deffacts seed (data 2) (data 1) (phase 1))
(defrule candidate
  (not (data ?x&=(* ?x ?x)))
  => (printout t "no fixed point" crlf))
(defrule drop-fixed-point
  (declare (salience -5))
  ?p <- (phase 1)
  ?d <- (data 1)
  => (retract ?p ?d) (printout t "retracted data 1" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
