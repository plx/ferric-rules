(deffacts d (a 1))
(defrule r (or ?f <- (a ?x)) => (retract ?f) (printout t "single " ?x crlf))
