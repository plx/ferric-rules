(deffacts test "rebinding of mulitfield vars"
   (_1 to see if the vars mess up if the fields are long)
   (_2 if so what is the limit also see if there is problem with bind))
(defrule ok 
   ?f1 <- (_1 $?one)
   ?f2 <- (_2 ? $?two)
   =>
   (retract ?f1 ?f2)
   (printout t "to see ... are long = " ?one  crlf)
   (printout t "if so ... with bind = "?two crlf)
   (bind ?one (create$ ?one (subseq$ ?two 1 10)))
   (printout t ?one crlf))
