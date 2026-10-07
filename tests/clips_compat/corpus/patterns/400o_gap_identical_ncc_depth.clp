(deffacts seed (item 1) (blocker) (other))
(defrule r1 (item ?x) (not (and (blocker) (other))) => (printout t r1 crlf))
(defrule r2 (item ?x) (not (and (blocker) (other))) => (printout t r2 crlf))
(defrule release (declare (salience 10)) ?f <- (blocker) => (retract ?f))

