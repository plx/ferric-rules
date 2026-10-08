(deffacts d (a 1) (b 2))
(defrule s (or ?f <- (a ?x) (b ?x)) => (retract ?f))
