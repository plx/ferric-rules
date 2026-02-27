(deffacts initial (factoid))
(defrule test
   ?fact <- (factoid)
   =>
   (printout t "any thing" crlf)
   (retract ?fact))
