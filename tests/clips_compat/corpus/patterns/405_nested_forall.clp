(deffacts d (a 1) (b 2) (c 1 2))
(defrule r (forall (a ?x) (forall (b ?y) (c ?x ?y))) => (printout t "nested_forall" crlf))
