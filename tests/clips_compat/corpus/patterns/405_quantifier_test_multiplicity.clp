(deffacts d (a 1) (b 1))
(defrule direct (declare (salience 30)) (or (test (eq 1 1)) (test (eq 2 2))) => (printout t OR crlf))
(defrule quantified (declare (salience 20)) (exists (or (test (eq 1 1)) (test (eq 2 2)))) => (printout t EXISTS crlf))
(defrule double (declare (salience 10)) (not (not (or (a 1) (b 1)))) => (printout t DOUBLE crlf))
