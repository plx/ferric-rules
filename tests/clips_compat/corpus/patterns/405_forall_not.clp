(deffacts d (a 1) (a 2))
(defrule r (forall (a ?x) (not (b ?x))) => (printout t "forall_not" crlf))
