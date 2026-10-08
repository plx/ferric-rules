(deffacts d (a 1) (b 1))
(defrule r (exists (forall (a ?x) (b ?x))) => (printout t exists_forall crlf))
