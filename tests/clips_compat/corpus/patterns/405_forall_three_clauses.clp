(deffacts d (a 1) (b 1) (c 1))
(defrule r (forall (a ?x) (b ?x) (c ?x)) => (printout t "forall_three_clauses" crlf))
