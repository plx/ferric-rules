(deffacts d (a 1) (a -1))
(defrule ft (forall (a ?x) (test (> ?x 0))) => (printout t unexpected crlf))
(defrule done (declare (salience -100)) => (printout t done crlf))
