(deffacts d (a 1) (b 2))
(defrule r (or (forall (a ?x) (test (> ?x 0))) (forall (b ?y) (test (> ?y 0)))) => (printout t branch crlf))
