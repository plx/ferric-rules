(deffacts seed (other) (item 1) (item 2) (item 3) (blocker a) (blocker b))
(defrule r (item ?x) (not (and (blocker ?) (other))) => (printout t ?x crlf))
(defrule remove-a (declare (salience 100)) ?b <- (blocker a) => (retract ?b))
(defrule remove-b (declare (salience 99)) ?b <- (blocker b) => (retract ?b))
