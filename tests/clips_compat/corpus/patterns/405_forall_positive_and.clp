(deffacts d (a 1) (enabled))
(defrule r (and (enabled) (forall (a ?x) (test (> ?x 0)))) => (printout t enabled crlf))
