(deffacts d (a 1) (b 2) (ready 1) (ready 2))
(defrule r (and (or (and (ready ?x) ?f <- (a ?x))
                       (and ?f <- (b ?x) (ready ?x)))) =>
  (printout t (fact-relation ?f) ":" ?x crlf) (retract ?f))
