(deffacts d (b 1) (c 1))
(defrule remove-b (declare (salience 10)) ?f <- (b 1) => (retract ?f) (printout t "removed b" crlf))
(defrule remove-c (declare (salience -10)) ?f <- (c 1) => (retract ?f) (printout t "removed c" crlf))
(defrule no (not (or (b 1) (c 1))) => (printout t clear crlf))
