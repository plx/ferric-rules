(deffacts d (a 1) (b 1) (a 2))
(defrule r (not (forall (a ?x) (b ?x))) => (printout t "not_forall" crlf))
