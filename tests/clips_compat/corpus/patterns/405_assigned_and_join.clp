(deffacts d (a 1) (a 2) (b 1) (b 2))
(defrule r (and ?a <- (a ?x) ?b <- (b ?x)) =>
  (retract ?a ?b) (printout t "pair " ?x crlf))
