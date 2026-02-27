(deftemplate a (field one))
(defrule a                         ; DR0627
   ?f1 <- (a (one two three))      ; DR0627
   =>                              ; DR0627
   (assert (not good)))
