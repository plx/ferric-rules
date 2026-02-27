(defrule test
   ?fact <- (initial-fact)
   =>
   (printout t "any thing" crlf)
   (retract ?fact))
