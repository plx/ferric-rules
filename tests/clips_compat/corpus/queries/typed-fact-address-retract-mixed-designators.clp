(deffacts seed (item))
(defrule run ?f <- (item) =>
  (retract 9 ?f -1)
  (printout t (fact-existp ?f) ":continued" crlf))
