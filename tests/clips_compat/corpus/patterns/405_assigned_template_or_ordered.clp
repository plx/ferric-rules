(deftemplate box (slot value))
(deffacts d (a 1) (box (value 2)))
(defrule r (or ?f <- (a ?x) ?f <- (box (value ?x))) =>
  (printout t (fact-relation ?f) ":" ?x crlf) (retract ?f))
