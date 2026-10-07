(deffacts seed (blocker) (a) (b))
(defrule clear (declare (salience 10)) ?b <- (blocker) => (retract ?b))
(defrule r1 (a) (not (blocker)) => (printout t r1 crlf))
(defrule r2 (b) (not (blocker)) => (printout t r2 crlf))
