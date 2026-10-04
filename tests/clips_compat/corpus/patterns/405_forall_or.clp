(deffacts d (a 1) (a 2) (b 1) (c 2))
(defrule r (forall (a ?x) (or (b ?x) (c ?x))) => (printout t "forall_or" crlf))
