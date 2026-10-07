(deffacts seed (item 1) (item 2) (item 3) (item 4) (item 5) (blocker) (other))
(defrule clear (declare (salience 10)) ?b <- (blocker) => (retract ?b))
(defrule r (item ?x) (not (and (blocker) (other))) => (printout t "r " ?x crlf))
