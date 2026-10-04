(deffacts d (a 1) (a 2))
(defrule ft (forall (a ?x) (test (> ?x 0))) => (printout t "ft fired" crlf))
