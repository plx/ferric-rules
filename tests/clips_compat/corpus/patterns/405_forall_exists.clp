(deffacts d (a 1) (b 1))
(defrule r (forall (a ?x) (exists (b ?x))) => (printout t "forall_exists" crlf))
