(deffacts d (a 1) (b 2))
(defrule r (or ?f <- (a ?x) ?f <- (b ?x)) =>
  (retract ?f) (printout t "retracted " ?x crlf))
