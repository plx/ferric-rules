(deffacts seed (a) (b) (blocker))
(defrule r1 (a) (not (blocker)) => (printout t r1 crlf))
(defrule r2 (b) (not (and (a) (not (blocker)))) => (printout t r2 crlf))
(defrule release (declare (salience 10)) ?b <- (blocker) => (retract ?b))
