(deffacts seed (item))
(deffunction boom () (printout t "evaluated" crlf) 1)
(defrule run ?f <- (item) =>
  (retract -1 ?f (boom))
  (printout t (fact-existp ?f) ":continued" crlf))
