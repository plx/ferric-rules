(deffacts seed (blocker 1) (blocker 2) (item 1) (item 2) (item 3))
(defrule r (item ?x) (not (blocker ?)) => (printout t ?x crlf))
(defrule remove-1 (declare (salience 100)) ?b <- (blocker 1) => (retract ?b))
(defrule remove-2 (declare (salience 99)) ?b <- (blocker 2) => (retract ?b))
