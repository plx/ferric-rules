(deffacts d (a 1) (a 2) (b 3) (ready 1) (ready 3))
(defrule r (or ?f <- (a ?x) ?f <- (b ?x)) (ready ?x) =>
  (retract ?f) (printout t "joined " ?x crlf))
