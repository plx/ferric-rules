(deffacts input
   (gift ball shoe food "candies  " 3 1 )
   (but we didn't have time !))
(defrule check 
   ?f1 <- (gift ?ball $?multi)
   ?f2 <- (but $?rest)
   =>
   (printout t "?ball = "?ball crlf "?multi " ?multi crlf)
   (printout t "but " ?rest crlf)
   (printout t "let's mess with them " crlf)
   (bind ?multi (create$ (subseq$ ?rest 1 3)))
   (printout t "we didn't have = " ?multi  crlf))
