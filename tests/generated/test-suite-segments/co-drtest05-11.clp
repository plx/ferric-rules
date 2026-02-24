(deftemplate a                     ; DR0460
   (field one) (field two))
(defrule one                       ; DR0460
   ?fact <- (a)
   =>
   (modify ?a (two)))
