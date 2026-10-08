(deffacts d (a 1) (b 2))
(defrule r (or ?f <- (a ?x) (b ?x)) => (printout t x " " ?x crlf))
