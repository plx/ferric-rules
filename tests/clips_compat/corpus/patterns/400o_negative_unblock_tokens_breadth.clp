(deffacts seed (item 1) (item 2) (item 3) (item 4) (item 5) (blocker))
(defrule clear (declare (salience 10)) ?b <- (blocker) => (retract ?b))
(defrule r (item ?x) (not (blocker)) => (printout t "r " ?x crlf))
