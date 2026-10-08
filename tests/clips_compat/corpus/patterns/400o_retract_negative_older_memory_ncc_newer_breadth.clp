(deffacts seed (blocker x) (other) (a 1) (a 2))
(defrule r1 (a ?n) (not (blocker ?)) => (printout t r1 " " ?n crlf))
(defrule r2 (a ?n) (not (and (blocker x) (other))) => (printout t r2 " " ?n crlf))
(defrule release (declare (salience 10)) ?f <- (blocker x) => (retract ?f))
