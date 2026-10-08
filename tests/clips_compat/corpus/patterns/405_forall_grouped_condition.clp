(deffacts d (a 1) (b 1))
(defrule r (forall (and (a ?x)) (b ?x)) => (printout t forall_grouped_condition crlf))
